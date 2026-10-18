//! `courses auth serve`: a local web page to sign in, check the session and
//! export/import the cookies. Bound to 127.0.0.1 only.

use crate::client::CarrefourClient;
use crate::session::{self, has_sso, load_jar, merge, parse_cookies, save_jar};
use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

const PAGE: &str = include_str!("serve.html");

// ponytail: one request at a time; a login blocks the server until it ends (single user).
pub fn serve(port: u16) -> Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    println!("Open http://127.0.0.1:{}  (Ctrl+C to stop)", port);
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        if let Err(e) = handle(&mut stream, port) {
            eprintln!("serve: {}", e);
        }
    }
    Ok(())
}

struct Req {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: String,
}

impl Req {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
}

fn read_req(stream: &TcpStream) -> Result<Req> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut r = BufReader::new(stream);
    let mut line = String::new();
    r.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let (method, path) = (parts.next().unwrap_or("").to_string(), parts.next().unwrap_or("").to_string());
    let mut headers = vec![];
    loop {
        let mut h = String::new();
        r.read_line(&mut h)?;
        match h.trim().split_once(':') {
            Some((k, v)) => headers.push((k.trim().to_lowercase(), v.trim().to_string())),
            None => break,
        }
    }
    let len: usize = headers
        .iter()
        .find(|(k, _)| k == "content-length")
        .and_then(|(_, v)| v.parse().ok())
        .unwrap_or(0);
    if len > 1 << 20 {
        bail!("request body too large");
    }
    let mut body = vec![0; len];
    r.read_exact(&mut body)?;
    Ok(Req { method, path, headers, body: String::from_utf8_lossy(&body).into() })
}

fn respond(stream: &mut TcpStream, status: u16, content_type: &str, body: &str) -> Result<()> {
    let reason = if status == 200 { "OK" } else { "Error" };
    write!(
        stream,
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\n\
         X-Frame-Options: DENY\r\nConnection: close\r\n\r\n{}",
        status,
        reason,
        content_type,
        body.len(),
        body
    )?;
    Ok(())
}

fn result(r: Result<Value>) -> (u16, Value) {
    match r {
        Ok(v) => (200, v),
        Err(e) => (500, json!({ "error": e.to_string() })),
    }
}

fn handle(stream: &mut TcpStream, port: u16) -> Result<()> {
    let req = read_req(stream)?;
    // Only answer to our own host name: blocks DNS rebinding.
    let host = req.header("host").unwrap_or("");
    let host_ok = host == format!("127.0.0.1:{}", port) || host == format!("localhost:{}", port);
    // A cross-site page cannot send a custom header without a CORS preflight,
    // which we never approve: blocks CSRF on the API.
    let api_ok = !req.path.starts_with("/api/") || req.header("x-courses-cli") == Some("1");
    if !host_ok || !api_ok {
        return respond(stream, 403, "text/plain", "forbidden");
    }

    let (status, body) = match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/") => return respond(stream, 200, "text/html; charset=utf-8", PAGE),
        ("GET", "/api/status") => result(status()),
        ("POST", "/api/login") => result(session::login().map(|c| json!({ "sso": has_sso(&c) }))),
        ("POST", "/api/refresh") => result(CarrefourClient::new().and_then(|c| c.refresh()).map(|_| json!({}))),
        ("GET", "/api/cookies") => result(load_jar().map(Value::from)),
        ("POST", "/api/cookies") => result(import(&req.body)),
        ("POST", "/api/logout") => result(crate::commands::auth::logout().map(|_| json!({}))),
        _ => (404, json!({ "error": "not found" })),
    };
    respond(stream, status, "application/json", &body.to_string())
}

fn status() -> Result<Value> {
    let jar = load_jar()?;
    let mut out = json!({ "cookies": jar.len(), "sso_cookie": has_sso(&jar) });
    if jar.is_empty() {
        return Ok(out);
    }
    let client = CarrefourClient::new()?;
    if let Ok(p) = client.get_me() {
        if p.email.is_some() || p.user_id.is_some() {
            let name = [p.first_name, p.last_name].into_iter().flatten().collect::<Vec<_>>().join(" ");
            out["name"] = json!(name);
            out["email"] = json!(p.email);
            out["loyalty"] = json!(p.loyalty_card.map(|c| c.number));
        }
    }
    if has_sso(&jar) {
        out["sso_alive"] = json!(client.refresh().is_ok());
    }
    Ok(out)
}

fn import(body: &str) -> Result<Value> {
    let new = parse_cookies(body);
    if new.is_empty() {
        bail!("No cookie found in the input.");
    }
    let mut jar = load_jar()?;
    let n = new.len();
    merge(&mut jar, new);
    save_jar(&jar)?;
    Ok(json!({ "imported": n, "sso": has_sso(&jar) }))
}
