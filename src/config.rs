// SPDX-License-Identifier: GPL-3.0-or-later
//! Configuration layout (all re-read automatically when any file changes):
//!
//!   <dir>/pi-status.toml       global settings and built-in collectors
//!   <dir>/services.d/*.toml    one file per systemd service
//!   <dir>/panels.d/*.toml      one file per custom command panel
//!
//! Drop-in files are processed in file-name order; anything not ending in
//! `.toml` (e.g. `tor.toml.disabled`) is ignored.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub const MAIN_FILE: &str = "pi-status.toml";
pub const SERVICES_DIR: &str = "services.d";
pub const PANELS_DIR: &str = "panels.d";

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Main {
    /// Address to bind. Changing this needs a restart.
    pub listen: String,
    pub title: String,
    /// Seconds between samples of the fast collectors (system, services, UPS, storage).
    pub interval: u64,
    /// Number of samples kept for the sparklines.
    pub history: usize,
    /// Seconds between re-running service version commands.
    pub version_refresh: u64,
    /// Serve UI files from this directory instead of the embedded copies (for tweaking the UI).
    pub ui_dir: Option<PathBuf>,
    pub ui: UiCfg,
    pub thresholds: Thresholds,
    pub system: SystemCfg,
    pub storage: Vec<MountCfg>,
    pub ups: UpsCfg,
    pub bitcoin: BitcoinCfg,
    pub logs: Vec<LogCfg>,
}

impl Default for Main {
    fn default() -> Self {
        Main {
            listen: "127.0.0.1:8080".into(),
            title: "Pi Status".into(),
            interval: 5,
            history: 120,
            version_refresh: 6 * 3600,
            ui_dir: None,
            ui: UiCfg::default(),
            thresholds: Thresholds::default(),
            system: SystemCfg::default(),
            storage: vec![MountCfg { path: "/".into(), label: Some("System".into()) }],
            ups: UpsCfg::default(),
            bitcoin: BitcoinCfg::default(),
            logs: Vec::new(),
        }
    }
}

/// Defaults for viewers who haven't picked a palette/theme in their browser.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct UiCfg {
    /// Built-in (mono, crimson, navy, forest, amber) or the id of one in `palettes`.
    pub palette: String,
    /// auto / dark / light
    pub theme: String,
    /// Extra palettes, added to (or overriding) the built-in ones.
    pub palettes: Vec<PaletteCfg>,
}

impl Default for UiCfg {
    fn default() -> Self {
        UiCfg { palette: "mono".into(), theme: "auto".into(), palettes: Vec::new() }
    }
}

/// A colour pair: `dark` is the background in dark mode and the text in light
/// mode; `light` is the reverse. Every other shade is derived from the pair.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PaletteCfg {
    pub id: String,
    pub name: String,
    pub dark: String,
    pub light: String,
    /// Alert colour in dark mode.
    pub accent_dark: String,
    /// Alert colour in light mode.
    pub accent_light: String,
}

fn is_hex_color(s: &str) -> bool {
    s.strip_prefix('#')
        .is_some_and(|h| matches!(h.len(), 3 | 6) && h.chars().all(|c| c.is_ascii_hexdigit()))
}

impl UiCfg {
    fn validate(&self, path: &Path, errors: &mut Vec<String>) {
        if !matches!(self.theme.as_str(), "auto" | "dark" | "light") {
            errors.push(format!("{}: [ui] theme must be auto, dark or light", path.display()));
        }
        for p in &self.palettes {
            for (field, v) in [("dark", &p.dark), ("light", &p.light), ("accent_dark", &p.accent_dark), ("accent_light", &p.accent_light)] {
                if !is_hex_color(v) {
                    errors.push(format!("{}: palette '{}': {field} = \"{v}\" is not a #rgb / #rrggbb colour", path.display(), p.id));
                }
            }
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Thresholds {
    pub temp_warn: f64,
    pub temp_crit: f64,
    pub cpu_warn: f64,
    pub mem_warn: f64,
    pub disk_warn: f64,
    pub disk_crit: f64,
    pub battery_warn: f64,
    pub battery_crit: f64,
}

impl Default for Thresholds {
    fn default() -> Self {
        Thresholds {
            temp_warn: 70.0,
            temp_crit: 80.0,
            cpu_warn: 90.0,
            mem_warn: 90.0,
            disk_warn: 85.0,
            disk_crit: 95.0,
            battery_warn: 30.0,
            battery_crit: 15.0,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SystemCfg {
    pub thermal_zone: String,
    /// Command printing `throttled=0x...`. Empty disables the check.
    pub throttled_cmd: String,
    /// Interface for the network tile. Empty sums the physical interfaces.
    pub net_iface: String,
}

impl Default for SystemCfg {
    fn default() -> Self {
        SystemCfg {
            thermal_zone: "/sys/class/thermal/thermal_zone0/temp".into(),
            throttled_cmd: "vcgencmd get_throttled".into(),
            net_iface: String::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MountCfg {
    pub path: String,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UpsCfg {
    pub enabled: bool,
    pub bus: u8,
    pub address: u16,
}

impl Default for UpsCfg {
    fn default() -> Self {
        UpsCfg { enabled: false, bus: 1, address: 0x2d }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BitcoinCfg {
    pub enabled: bool,
    pub rpc_url: String,
    /// Used when `user`/`password` are not set.
    pub cookie_file: Option<PathBuf>,
    pub user: Option<String>,
    pub password: Option<String>,
    pub interval: u64,
    pub timeout: u64,
}

impl Default for BitcoinCfg {
    fn default() -> Self {
        BitcoinCfg {
            enabled: false,
            rpc_url: "http://127.0.0.1:8332".into(),
            cookie_file: None,
            user: None,
            password: None,
            interval: 30,
            timeout: 5,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogCfg {
    pub id: String,
    pub name: String,
    /// Arguments passed to journalctl.
    pub args: Vec<String>,
    /// Keep only lines containing one of these (case-insensitive).
    #[serde(default)]
    pub include: Vec<String>,
    /// Drop lines containing any of these (case-insensitive).
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default = "default_log_interval")]
    pub interval: u64,
    #[serde(default = "default_max_lines")]
    pub max_lines: usize,
    /// Raise a warning on the dashboard while the log is non-empty.
    #[serde(default)]
    pub alert: bool,
}

fn default_log_interval() -> u64 {
    60
}
fn default_max_lines() -> usize {
    500
}
fn default_true() -> bool {
    true
}
fn default_group() -> String {
    "Services".into()
}
fn default_panel_interval() -> u64 {
    60
}
fn default_timeout() -> u64 {
    10
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceCfg {
    #[serde(skip)]
    pub id: String,
    pub name: String,
    pub unit: String,
    #[serde(default = "default_group")]
    pub group: String,
    /// Optional URL shown as a link on the service.
    pub link: Option<String>,
    pub version: Option<VersionCfg>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct VersionCfg {
    /// Shell command; the first `x.y[.z[.w]]` in its stdout+stderr becomes the version.
    pub cmd: Option<String>,
    /// Static version string, used instead of `cmd`.
    pub fixed: Option<String>,
    /// Use the trimmed first line of output as-is instead of extracting a number.
    #[serde(default)]
    pub raw: bool,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum PanelFormat {
    /// Lines of `key : value` / `key = value`, rendered as rows.
    Kv,
    /// Free-form monospace text.
    Text,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelCfg {
    #[serde(skip)]
    pub id: String,
    pub name: String,
    pub cmd: String,
    #[serde(default = "default_format")]
    pub format: PanelFormat,
    #[serde(default = "default_panel_interval")]
    pub interval: u64,
    #[serde(default = "default_timeout")]
    pub timeout: u64,
    /// Larger 2×2 card instead of the default 2×1.
    #[serde(default)]
    pub wide: bool,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_format() -> PanelFormat {
    PanelFormat::Text
}

#[derive(Debug, Clone)]
pub struct Config {
    pub main: Main,
    pub services: Vec<ServiceCfg>,
    pub panels: Vec<PanelCfg>,
    /// Problems with individual files; shown on the dashboard.
    pub errors: Vec<String>,
}

impl Config {
    /// Load the whole config directory. A broken main file is fatal on first
    /// load; on reload the previous main settings are kept and the error reported.
    /// A broken drop-in file only skips that file.
    pub fn load(dir: &Path, previous: Option<&Config>) -> Result<Config, String> {
        let mut errors = Vec::new();

        let main_path = dir.join(MAIN_FILE);
        let main = match read_toml::<Main>(&main_path) {
            Ok(m) => m,
            Err(e) => match previous {
                Some(p) => {
                    errors.push(format!("{e}\n(keeping previous settings)"));
                    p.main.clone()
                }
                None => return Err(e),
            },
        };

        main.ui.validate(&main_path, &mut errors);

        let t = &main.thresholds;
        for (name, warn, crit) in [("temp", t.temp_warn, t.temp_crit), ("disk", t.disk_warn, t.disk_crit)] {
            if warn > crit {
                errors.push(format!("{}: [thresholds] {name}_warn ({warn}) is above {name}_crit ({crit})", main_path.display()));
            }
        }
        if t.battery_warn < t.battery_crit {
            errors.push(format!("{}: [thresholds] battery_warn should be above battery_crit", main_path.display()));
        }

        let mut seen = std::collections::HashSet::new();
        for l in &main.logs {
            if !seen.insert(l.id.as_str()) {
                errors.push(format!("{}: duplicate log id '{}'", main_path.display(), l.id));
            }
            if l.id.is_empty() || !l.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
                errors.push(format!("{}: log id '{}' may only contain letters, digits, '-' and '_'", main_path.display(), l.id));
            }
        }

        let services = load_dropins::<ServiceCfg>(&dir.join(SERVICES_DIR), &mut errors)
            .into_iter()
            .filter_map(|(id, mut s)| {
                if s.unit.trim().is_empty() {
                    errors.push(format!("{SERVICES_DIR}/{id}.toml: 'unit' is empty"));
                    return None;
                }
                s.id = id;
                s.enabled.then_some(s)
            })
            .collect();

        let panels = load_dropins::<PanelCfg>(&dir.join(PANELS_DIR), &mut errors)
            .into_iter()
            .filter_map(|(id, mut p)| {
                if p.cmd.trim().is_empty() {
                    errors.push(format!("{PANELS_DIR}/{id}.toml: 'cmd' is empty"));
                    return None;
                }
                p.id = id;
                p.enabled.then_some(p)
            })
            .collect();

        Ok(Config { main, services, panels, errors })
    }
}

fn read_toml<T: DeserializeOwned>(path: &Path) -> Result<T, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    toml::from_str(&text).map_err(|e| format!("{}: {}", path.display(), e.to_string().trim_end()))
}

fn dropin_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = match fs::read_dir(dir) {
        Ok(rd) => rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "toml") && p.is_file())
            .collect(),
        Err(_) => Vec::new(),
    };
    files.sort();
    files
}

fn load_dropins<T: DeserializeOwned>(dir: &Path, errors: &mut Vec<String>) -> Vec<(String, T)> {
    dropin_files(dir)
        .into_iter()
        .filter_map(|path| {
            let id = path.file_stem()?.to_string_lossy().into_owned();
            match read_toml::<T>(&path) {
                Ok(v) => Some((id, v)),
                Err(e) => {
                    errors.push(e);
                    None
                }
            }
        })
        .collect()
}

/// Cheap fingerprint of the config directory, used to detect edits.
pub fn signature(dir: &Path) -> Vec<(PathBuf, Option<SystemTime>)> {
    let mut paths = vec![dir.join(MAIN_FILE)];
    for sub in [SERVICES_DIR, PANELS_DIR] {
        if let Ok(rd) = fs::read_dir(dir.join(sub)) {
            paths.extend(rd.flatten().map(|e| e.path()));
        }
    }
    let mut sig: Vec<_> = paths
        .into_iter()
        .map(|p| {
            let m = fs::metadata(&p).and_then(|m| m.modified()).ok();
            (p, m)
        })
        .collect();
    sig.sort();
    sig
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_dir(name: &str, main: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pi-status-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(SERVICES_DIR)).unwrap();
        fs::write(dir.join(MAIN_FILE), main).unwrap();
        dir
    }

    #[test]
    fn hex_colours() {
        assert!(is_hex_color("#fff") && is_hex_color("#1a0F2e"));
        assert!(!is_hex_color("fff") && !is_hex_color("#ffff") && !is_hex_color("#ggg") && !is_hex_color("red"));
    }

    #[test]
    fn validation_reports_problems_without_failing() {
        let dir = write_dir(
            "validate",
            r##"
[thresholds]
temp_warn = 90
temp_crit = 80

[ui]
theme = "neon"

[[ui.palettes]]
id = "x"
name = "X"
dark = "#000"
light = "white"
accent_dark = "#f00"
accent_light = "#f00"

[[logs]]
id = "bad id"
name = "Bad"
args = ["-b"]
"##,
        );
        fs::write(dir.join(SERVICES_DIR).join("10-ok.toml"), "name = \"A\"\nunit = \"a\"\n").unwrap();
        fs::write(dir.join(SERVICES_DIR).join("20-broken.toml"), "name = \"B\"\nunit = \n").unwrap();
        fs::write(dir.join(SERVICES_DIR).join("30-off.toml.disabled"), "garbage").unwrap();

        let c = Config::load(&dir, None).expect("main file parses");
        let all = c.errors.join("\n");
        assert!(all.contains("temp_warn"), "{all}");
        assert!(all.contains("theme"), "{all}");
        assert!(all.contains("light = \"white\""), "{all}");
        assert!(all.contains("log id 'bad id'"), "{all}");
        assert!(all.contains("20-broken.toml"), "{all}");
        assert_eq!(c.services.len(), 1, "broken and disabled files are skipped");
        assert_eq!(c.services[0].id, "10-ok");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn broken_main_file_keeps_previous_settings_on_reload() {
        let dir = write_dir("reload", "title = \"First\"\n");
        let first = Config::load(&dir, None).unwrap();
        fs::write(dir.join(MAIN_FILE), "title = \n").unwrap();
        assert!(Config::load(&dir, None).is_err(), "fatal on first load");
        let reloaded = Config::load(&dir, Some(&first)).unwrap();
        assert_eq!(reloaded.main.title, "First");
        assert_eq!(reloaded.errors.len(), 1);
        fs::remove_dir_all(&dir).ok();
    }
}
