# pi-status

A status dashboard for a Raspberry Pi home server in a single binary: Rust, ~1.4 MB static, with the UI and fonts embedded.

## What it shows

* **Vitals:** CPU, temperature, memory, network and load, each with a 10-minute history, plus uptime.
* **Services:** state, uptime, memory, restarts and version for every systemd unit you list.
* **Bitcoin node:** block height, sync progress, peers, mempool and fees over JSON-RPC.
* **Power:** battery, input and cell voltages from a Waveshare UPS HAT (E), plus the Pi's
  under-voltage and throttling flags.
* **Storage:** usage for the mounts you list.
* **Logs:** journal views, for example kernel warnings, with filtering.
* **Custom panels:** the output of any command as a card.
* **Alerts:** a single summary of everything that needs attention, with configurable thresholds.

## How it works

* **One static binary** with the web UI and fonts embedded. No runtime, interpreter or web
  server needed on the Pi.
* **Background collectors** sample on a timer. Page loads only read the latest snapshot, so
  any number of viewers costs nothing extra.
* **Direct sources:** reads `/proc`, `/sys` and `statvfs` itself, talks to I²C and bitcoind
  directly, and asks systemd for all services in a single call.
* **Plain TOML config:** one file per service or panel. Changes are picked up within seconds,
  with no rebuild and no restart.

## Configuration

```
/etc/pi-status/
├── pi-status.toml         global settings, thresholds, storage, UPS, bitcoin, logs
├── services.d/            one file per systemd service
│   ├── 10-bitcoind.toml
│   ├── 20-tor.toml
│   └── …
└── panels.d/              optional: any command's output as a card
    └── 10-top-processes.toml
```

* **Hot reload:** every file is re-read when it changes. Only `listen` needs a restart.
* **Order:** services are shown in file-name order and grouped by `group`.
* **Disable:** rename to `*.toml.disabled`, or set `enabled = false`.
* **Errors:** a broken file is skipped and the problem is shown on the dashboard. The rest keeps working.
* **Validate:** `pi-status --check -c /etc/pi-status`
* **Debug:** `pi-status --once -c /etc/pi-status` prints the full JSON snapshot.

### A service

```toml
# services.d/30-vaultwarden.toml
name  = "Vaultwarden"
unit  = "vaultwarden"        # systemd unit
group = "Apps"               # section heading on the dashboard
link  = "https://vault.example.lan"   # optional

[version]
cmd = "/opt/vaultwarden/vaultwarden --version"   # first x.y.z in stdout+stderr
# fixed = "0.1"     # static version instead of a command
# raw   = true      # show the first output line verbatim
```

### A custom panel

```toml
# panels.d/20-wireguard.toml
name     = "WireGuard"
cmd      = "wg show wg0 latest-handshakes"
format   = "text"   # or "kv" for "key : value" lines
interval = 60
```

### UI

Inspired by Nothing's design language: two colours plus one accent, which marks only the
thing in trouble. Dot-matrix type (Doto) is used for numbers, and every graphic (history
charts, meters, the battery ring) is drawn in dots. Cards sit on a bento grid of square units:
2 columns on phones, 4 on tablets and 6 on desktops.

Cards never scroll on their own, so the page always scrolls under your cursor or finger.
Content that doesn't fit fades out at the bottom. Tap a custom panel, a log row or a service
to open it in full.

**Palettes.** A palette is a colour pair plus an alert accent. Dark mode uses the dark colour
as background and the light one as text; light mode swaps them. Every other shade
(surfaces, lines, unlit dots, secondary text) is derived from the pair, so any palette
stays consistent. Built in: Mono, Crimson, Navy, Forest and Amber. Viewers pick one, and
auto/dark/light, from the header (remembered per browser). The default and any custom
palettes live in `pi-status.toml`:

```toml
[ui]
palette = "crimson"
theme   = "auto"

[[ui.palettes]]
id           = "violet"
name         = "Violet"
dark         = "#1a0f2e"   # background in dark mode, text in light mode
light        = "#efe6ff"   # text in dark mode, background in light mode
accent_dark  = "#ffd43b"
accent_light = "#c2410c"
```
Fonts are embedded in the binary. They are all under the SIL Open Font License; see
`ui/fonts/README` and the `OFL-*.txt` files next to it.

### UI tweaks without rebuilding

Set `ui_dir = "/etc/pi-status/ui"` and copy `ui/*` there. Files in that directory override the
embedded ones on the next page load.

## API

| Endpoint | |
|---|---|
| `GET api/status` | full snapshot (system, history, services, bitcoin, ups, storage, logs summary, panels, alerts) |
| `GET api/logs/<id>` | full lines of one log source |
| `POST api/versions/refresh` | re-run all version commands now |

Paths are matched on `/api/…` anywhere in the URL, and the UI uses relative URLs, so it
works behind a reverse proxy under a path prefix. For example, to serve it at `/status`:

```nginx
location = /status { return 301 /status/; }   # the trailing slash matters (relative URLs)
location /status/  { proxy_pass http://127.0.0.1:8080/; }
```

## Build

From the x86 desktop (static binary, no C toolchain needed):

```bash
rustup target add aarch64-unknown-linux-musl
cargo build --release --target aarch64-unknown-linux-musl
# → target/aarch64-unknown-linux-musl/release/pi-status
```

Or build natively on the Pi with `cargo build --release`.

## Install on the Pi

```bash
scp target/aarch64-unknown-linux-musl/release/pi-status <pi>:/tmp/
scp -r config <pi>:/tmp/pi-status-config
scp deploy/pi-status.service <pi>:/tmp/
```

Then on the Pi:

```bash
sudo useradd --system --no-create-home --shell /usr/sbin/nologin pi-status
sudo install -m 0755 /tmp/pi-status /usr/local/bin/pi-status
sudo mkdir -p /etc/pi-status && sudo cp -r /tmp/pi-status-config/. /etc/pi-status/
sudo chown -R root:pi-status /etc/pi-status && sudo chmod 0640 /etc/pi-status/pi-status.toml
sudo -u pi-status pi-status --check -c /etc/pi-status
sudo cp /tmp/pi-status.service /etc/systemd/system/pi-status.service
sudo systemctl daemon-reload && sudo systemctl enable --now pi-status
```

### Read-only Bitcoin RPC user

Generate credentials with Knots' `share/rpcauth/rpcauth.py pistatus`, then add to `bitcoin.conf`:

```
rpcauth=pistatus:<salt>$<hash>
rpcwhitelist=pistatus:getblockchaininfo,getnetworkinfo,getmempoolinfo
rpcwhitelistdefault=0
```

`rpcwhitelistdefault=0` matters. Without it, setting any `rpcwhitelist` makes every *other* RPC
user (electrs, mempool) default to an empty whitelist, and they lose access.

Put the password in `[bitcoin]` in `pi-status.toml` and restart bitcoind.

### Permissions

The service runs as the unprivileged `pi-status` user with these groups:

* `video` for `vcgencmd`
* `i2c` for the UPS
* `systemd-journal` for the log panels

If a version command needs root (like `ufw --version`), use `dpkg-query -W -f='${Version}' <pkg>`
instead, as the shipped configs do.

## Behaviour notes

* **Network tile:** with `net_iface = ""` it sums physical interfaces only. Bridges, veth pairs
  and tunnels are skipped so traffic isn't counted twice.
* **Logs:** sources without `include`/`exclude` filters ask journalctl for just the last
  `max_lines` lines. Filtered sources read the whole range given by `args`, so keep that bounded
  (for example `-b` for the current boot).
* **Commands:** every command (versions, panels, `vcgencmd`, `systemctl`, `journalctl`) has a
  timeout and runs in its own process group, which is killed on timeout. Output is capped at 4 MiB.
* **No home directory needed:** the service user has none, but some tools create dot-folders
  even for `--version` (bitcoind, Qt apps like qbittorrent-nox). When `$HOME` isn't writable,
  commands get a private scratch home under `/tmp`. If a version still can't be read, the
  service's sheet shows the reason.
* **Resilience:** a panic in one collector is logged and that collector keeps running. The
  rest of the dashboard is unaffected.
* **Storage:** `statvfs` on an unresponsive network mount can block. List local mounts only.

## License

pi-status is free software: you can redistribute it and/or modify it under the terms of the
GNU General Public License as published by the Free Software Foundation, either version 3 of
the License, or (at your option) any later version. See [LICENSE](LICENSE).

The embedded fonts (Doto, Space Grotesk, Space Mono) are separate works under the SIL Open
Font License 1.1, which allows bundling them with software under any licence. Their licence
texts are in [`ui/fonts/`](ui/fonts/).
