// SPDX-License-Identifier: GPL-3.0-or-later
use crate::state::Shared;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;
use tiny_http::{Header, Method, Request, Response, Server};

/// UI files compiled into the binary: (served name, path under ui/, content type, bytes).
/// `ui_dir` in the config can override any of them.
const ASSETS: &[(&str, &str, &str, &[u8])] = &[
    ("index.html", "index.html", "text/html; charset=utf-8", include_bytes!("../ui/index.html")),
    ("app.css", "app.css", "text/css; charset=utf-8", include_bytes!("../ui/app.css")),
    ("app.js", "app.js", "application/javascript; charset=utf-8", include_bytes!("../ui/app.js")),
    ("favicon.svg", "favicon.svg", "image/svg+xml", include_bytes!("../ui/favicon.svg")),
    ("doto.woff2", "fonts/doto.woff2", "font/woff2", include_bytes!("../ui/fonts/doto.woff2")),
    ("space-grotesk.woff2", "fonts/space-grotesk.woff2", "font/woff2", include_bytes!("../ui/fonts/space-grotesk.woff2")),
    ("space-mono-400.woff2", "fonts/space-mono-400.woff2", "font/woff2", include_bytes!("../ui/fonts/space-mono-400.woff2")),
    ("space-mono-700.woff2", "fonts/space-mono-700.woff2", "font/woff2", include_bytes!("../ui/fonts/space-mono-700.woff2")),
];

const WORKERS: usize = 4;

pub fn serve(shared: Arc<Shared>, listen: &str) -> Result<(), String> {
    let server = Arc::new(Server::http(listen).map_err(|e| e.to_string())?);
    let workers: Vec<_> = (0..WORKERS)
        .map(|_| {
            let (server, shared) = (server.clone(), shared.clone());
            thread::spawn(move || {
                for req in server.incoming_requests() {
                    handle(&shared, req);
                }
            })
        })
        .collect();
    for w in workers {
        let _ = w.join();
    }
    Ok(())
}

fn header(k: &str, v: &str) -> Header {
    Header::from_bytes(k.as_bytes(), v.as_bytes()).expect("valid header")
}

fn json(status: u16, body: String) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_data(body.into_bytes())
        .with_status_code(status)
        .with_header(header("Content-Type", "application/json; charset=utf-8"))
        .with_header(header("Cache-Control", "no-store"))
}

fn handle(shared: &Shared, req: Request) {
    let url = req.url().split('?').next().unwrap_or("/").to_string();
    let method = req.method().clone();

    // Works both at the root and behind a reverse proxy that keeps a path prefix.
    let resp = if let Some(i) = url.find("/api/") {
        let api = &url[i + 5..];
        // HEAD is GET without a body (tiny_http drops the body for HEAD)
        let method = if method == Method::Head { Method::Get } else { method };
        match (&method, api) {
            (Method::Get, "status") => json(200, shared.snapshot().to_string()),
            (Method::Get, p) if p.starts_with("logs/") => match shared.log(&p[5..]) {
                Some(v) => json(200, v.to_string()),
                None => json(404, r#"{"error":"no such log"}"#.into()),
            },
            (Method::Post, "versions/refresh") => {
                shared.refresh_versions.store(true, Ordering::SeqCst);
                json(202, r#"{"ok":true}"#.into())
            }
            // a known endpoint with the wrong method is 405, not 404
            (_, "status") => json(405, r#"{"error":"method not allowed"}"#.into()).with_header(header("Allow", "GET")),
            (_, p) if p.starts_with("logs/") => json(405, r#"{"error":"method not allowed"}"#.into()).with_header(header("Allow", "GET")),
            (_, "versions/refresh") => json(405, r#"{"error":"method not allowed"}"#.into()).with_header(header("Allow", "POST")),
            _ => json(404, r#"{"error":"not found"}"#.into()),
        }
    } else if method == Method::Get || method == Method::Head {
        let name = url.rsplit('/').next().filter(|n| !n.is_empty()).unwrap_or("index.html");
        match asset(shared, name) {
            Some((ct, body)) => {
                let cache = if ct.starts_with("font/") { "public, max-age=86400" } else { "no-cache" };
                Response::from_data(body)
                    .with_header(header("Content-Type", ct))
                    .with_header(header("Cache-Control", cache))
            }
            None => Response::from_data(b"Not found".to_vec()).with_status_code(404),
        }
    } else {
        Response::from_data(b"Method not allowed".to_vec()).with_status_code(405)
    };

    let _ = req.respond(resp);
}

fn asset(shared: &Shared, name: &str) -> Option<(&'static str, Vec<u8>)> {
    let &(_, rel, ct, embedded) = ASSETS.iter().find(|(n, ..)| *n == name)?;
    if let Some(dir) = &shared.config().main.ui_dir {
        if let Ok(b) = std::fs::read(dir.join(rel)) {
            return Some((ct, b));
        }
    }
    Some((ct, embedded.to_vec()))
}
