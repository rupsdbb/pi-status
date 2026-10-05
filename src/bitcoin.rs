// SPDX-License-Identifier: GPL-3.0-or-later
//! Minimal JSON-RPC client for bitcoind (one batched HTTP/1.0 request per poll).

use crate::config::BitcoinCfg;
use serde::Serialize;
use serde_json::{json, Value};
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

#[derive(Serialize, Clone, Default)]
pub struct Bitcoin {
    pub chain: String,
    pub blocks: u64,
    pub headers: u64,
    pub progress: f64,
    pub ibd: bool,
    pub pruned: bool,
    pub size_on_disk: u64,
    pub best_block_time: u64,
    pub subversion: String,
    pub peers: u64,
    pub peers_in: u64,
    pub peers_out: u64,
    pub mempool_tx: u64,
    pub mempool_bytes: u64,
    pub mempool_usage: u64,
    pub mempool_max: u64,
    /// sat/vB
    pub mempool_min_fee: f64,
    pub warnings: Vec<String>,
}

fn base64(input: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for c in input.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

fn credentials(cfg: &BitcoinCfg) -> Result<String, String> {
    if let (Some(u), Some(p)) = (&cfg.user, &cfg.password) {
        return Ok(format!("{u}:{p}"));
    }
    let path = cfg.cookie_file.as_ref().ok_or("set user/password or cookie_file under [bitcoin]")?;
    fs::read_to_string(path)
        .map(|s| s.trim().to_string())
        .map_err(|e| format!("cannot read {}: {e}", path.display()))
}

fn host_port(url: &str) -> Result<(String, u16), String> {
    let rest = url.strip_prefix("http://").ok_or("rpc_url must start with http://")?;
    let hostport = rest.split('/').next().unwrap_or(rest);
    match hostport.rsplit_once(':') {
        Some((h, p)) => Ok((h.trim_matches(['[', ']']).to_string(), p.parse().map_err(|_| "bad port in rpc_url")?)),
        None => Ok((hostport.to_string(), 8332)),
    }
}

fn rpc_batch(cfg: &BitcoinCfg, methods: &[&str]) -> Result<Vec<Value>, String> {
    let (host, port) = host_port(&cfg.rpc_url)?;
    let auth = base64(credentials(cfg)?.as_bytes());
    let timeout = Duration::from_secs(cfg.timeout.max(1));

    let body: Vec<Value> = methods
        .iter()
        .enumerate()
        .map(|(i, m)| json!({"jsonrpc": "1.0", "id": i, "method": m, "params": []}))
        .collect();
    let body = Value::Array(body).to_string();

    // Try every resolved address: "localhost" may resolve to ::1 first while
    // bitcoind only listens on 127.0.0.1.
    let addrs: Vec<_> = (host.as_str(), port).to_socket_addrs().map_err(|e| format!("{host}:{port}: {e}"))?.collect();
    let mut last_err = format!("{host}:{port}: no address");
    let mut stream = None;
    for addr in &addrs {
        match TcpStream::connect_timeout(addr, timeout) {
            Ok(s) => {
                stream = Some(s);
                break;
            }
            Err(e) => last_err = format!("connect {addr}: {e}"),
        }
    }
    let mut s = stream.ok_or(last_err)?;
    s.set_read_timeout(Some(timeout)).ok();
    s.set_write_timeout(Some(timeout)).ok();

    write!(
        s,
        "POST / HTTP/1.0\r\nHost: {host}\r\nAuthorization: Basic {auth}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .map_err(|e| e.to_string())?;

    let mut resp = Vec::new();
    s.read_to_end(&mut resp).map_err(|e| format!("reading response: {e}"))?;
    let resp = String::from_utf8_lossy(&resp);
    let (head, body) = resp.split_once("\r\n\r\n").ok_or("malformed HTTP response")?;
    let status: u16 = head.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
    if status == 401 {
        return Err("RPC authentication failed (check user/password or cookie_file)".into());
    }
    if status == 403 {
        return Err("RPC call forbidden (check rpcwhitelist)".into());
    }

    let parsed: Value = serde_json::from_str(body).map_err(|_| format!("HTTP {status}: {}", body.trim()))?;
    let mut items = match parsed {
        Value::Array(a) => a,
        other => {
            let msg = other["error"]["message"].as_str().unwrap_or("unexpected response").to_string();
            return Err(msg);
        }
    };
    items.sort_by_key(|v| v["id"].as_u64().unwrap_or(u64::MAX));

    if items.len() != methods.len() {
        return Err(format!("expected {} RPC results, got {}", methods.len(), items.len()));
    }
    items
        .into_iter()
        .zip(methods)
        .map(|(v, m)| match v["error"]["message"].as_str() {
            Some(msg) => Err(format!("{m}: {msg}")),
            None => Ok(v["result"].clone()),
        })
        .collect()
}

fn warnings(v: &Value) -> Vec<String> {
    match v {
        Value::String(s) if !s.is_empty() => vec![s.clone()],
        Value::Array(a) => a.iter().filter_map(|x| x.as_str()).filter(|s| !s.is_empty()).map(String::from).collect(),
        _ => Vec::new(),
    }
}

pub fn collect(cfg: &BitcoinCfg) -> Result<Bitcoin, String> {
    let r = rpc_batch(cfg, &["getblockchaininfo", "getnetworkinfo", "getmempoolinfo"])?;
    let [chain, net, mp] = &r[..] else { unreachable!("rpc_batch checks the result count") };
    let u = |v: &Value| v.as_u64().unwrap_or(0);

    let mut w = warnings(&chain["warnings"]);
    for x in warnings(&net["warnings"]) {
        if !w.contains(&x) {
            w.push(x);
        }
    }

    Ok(Bitcoin {
        chain: chain["chain"].as_str().unwrap_or("").into(),
        blocks: u(&chain["blocks"]),
        headers: u(&chain["headers"]),
        progress: chain["verificationprogress"].as_f64().unwrap_or(0.0),
        ibd: chain["initialblockdownload"].as_bool().unwrap_or(false),
        pruned: chain["pruned"].as_bool().unwrap_or(false),
        size_on_disk: u(&chain["size_on_disk"]),
        best_block_time: chain["time"].as_u64().or(chain["mediantime"].as_u64()).unwrap_or(0),
        subversion: net["subversion"].as_str().unwrap_or("").trim_matches('/').into(),
        peers: u(&net["connections"]),
        peers_in: u(&net["connections_in"]),
        peers_out: u(&net["connections_out"]),
        mempool_tx: u(&mp["size"]),
        mempool_bytes: u(&mp["bytes"]),
        mempool_usage: u(&mp["usage"]),
        mempool_max: u(&mp["maxmempool"]),
        mempool_min_fee: mp["mempoolminfee"].as_f64().unwrap_or(0.0) * 1e8 / 1000.0,
        warnings: w,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn b64() {
        assert_eq!(super::base64(b"user:pass"), "dXNlcjpwYXNz");
        assert_eq!(super::base64(b"ab"), "YWI=");
        assert_eq!(super::base64(b"a"), "YQ==");
    }
}

#[cfg(test)]
mod url_tests {
    use super::host_port;

    #[test]
    fn parses_rpc_urls() {
        assert_eq!(host_port("http://127.0.0.1:8332").unwrap(), ("127.0.0.1".into(), 8332));
        assert_eq!(host_port("http://node.lan").unwrap(), ("node.lan".into(), 8332));
        assert_eq!(host_port("http://[::1]:18443/wallet/x").unwrap(), ("::1".into(), 18443));
        assert!(host_port("https://127.0.0.1:8332").is_err());
        assert!(host_port("http://host:notaport").is_err());
    }
}
