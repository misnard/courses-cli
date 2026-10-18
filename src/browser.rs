//! Every carrefour.fr call runs as a `fetch` inside a real Chrome page.
//!
//! Cloudflare answers plain HTTP clients with `403 cf-mitigated: challenge`
//! (reqwest/curl included), Leclerc's DataDome with a captcha; a full Chrome
//! in new-headless mode, with the `HeadlessChrome` UA masked and
//! `AutomationControlled` disabled, gets a 200 from both.
//! We talk to it with a minimal hand-rolled CDP client.

use anyhow::{anyhow, bail, Context};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use tungstenite::{stream::MaybeTlsStream, Message, WebSocket};

type Res<T> = anyhow::Result<T>;

pub fn chrome_path() -> Res<PathBuf> {
    if let Ok(p) = std::env::var("COURSES_CHROME") {
        return Ok(p.into());
    }
    let mut candidates: Vec<PathBuf> = [
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
    ]
    .map(PathBuf::from)
    .to_vec();
    for dir in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
        for name in ["google-chrome", "google-chrome-stable", "chromium", "chromium-browser"] {
            candidates.push(dir.join(name));
        }
    }
    candidates
        .into_iter()
        .find(|p| p.is_file())
        .ok_or_else(|| anyhow!("Chrome not found; set COURSES_CHROME to its executable"))
}

/// Chrome's own (reduced) UA without "HeadlessChrome". Given as a flag, not a
/// CDP override: the override leaves the client-hint brands empty or made up,
/// and DataDome answers that with a captcha.
fn user_agent(chrome: &Path) -> Res<String> {
    if let Ok(ua) = std::env::var("COURSES_USER_AGENT") {
        return Ok(ua);
    }
    let out = Command::new(chrome).arg("--version").output().context("cannot run Chrome")?;
    let version = String::from_utf8_lossy(&out.stdout);
    let major = version
        .split_whitespace()
        .find_map(|w| w.split('.').next().filter(|m| m.parse::<u32>().is_ok()).map(String::from))
        .ok_or_else(|| anyhow!("unexpected 'chrome --version' output: {version}"))?;
    let os = match std::env::consts::OS {
        "macos" => "Macintosh; Intel Mac OS X 10_15_7",
        "windows" => "Windows NT 10.0; Win64; x64",
        _ => "X11; Linux x86_64",
    };
    Ok(format!("Mozilla/5.0 ({os}) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{major}.0.0.0 Safari/537.36"))
}

pub fn spawn_chrome(profile: &Path, port: u16, headless: bool, url: &str) -> Res<Child> {
    let chrome = chrome_path()?;
    let mut cmd = Command::new(&chrome);
    cmd.arg(format!("--user-data-dir={}", profile.display()))
        .arg(format!("--remote-debugging-port={port}"))
        .args(["--no-first-run", "--no-default-browser-check", "--lang=fr-FR"]);
    if headless {
        cmd.args(["--headless=new", "--disable-blink-features=AutomationControlled"])
            .arg(format!("--user-agent={}", user_agent(&chrome)?));
    }
    cmd.arg(url)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("cannot start Chrome")
}

/// GET on the DevTools HTTP endpoint. Unlike a CDP session it attaches nothing
/// to the page, so Turnstile does not see an automated browser.
pub fn devtools_get(port: u16, path: &str) -> Res<Value> {
    use std::io::BufRead;
    let mut s = TcpStream::connect(("127.0.0.1", port))?;
    s.set_read_timeout(Some(Duration::from_secs(5)))?;
    // Chrome 155+ drops HTTP/1.0 and keeps the socket open: read Content-Length bytes.
    write!(s, "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n")?;
    let mut r = std::io::BufReader::new(s);
    let mut len = 0;
    loop {
        let mut line = String::new();
        r.read_line(&mut line)?;
        if line.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            if k.eq_ignore_ascii_case("content-length") {
                len = v.trim().parse()?;
            }
        }
    }
    let mut body = vec![0; len];
    r.read_exact(&mut body)?;
    Ok(serde_json::from_slice(&body)?)
}

pub struct Cdp {
    ws: WebSocket<MaybeTlsStream<TcpStream>>,
    next: u64,
    events: Vec<Value>,
}

impl Cdp {
    pub fn connect(ws_url: &str) -> Res<Cdp> {
        let (ws, _) = tungstenite::connect(ws_url).context("CDP connect")?;
        if let MaybeTlsStream::Plain(s) = ws.get_ref() {
            // Doubles as navigation and request timeout.
            let _ = s.set_read_timeout(Some(Duration::from_secs(60)));
        }
        Ok(Cdp { ws, next: 0, events: vec![] })
    }

    fn read(&mut self) -> Res<Value> {
        loop {
            match self.ws.read().context("CDP read")? {
                Message::Text(t) => return Ok(serde_json::from_str(&t)?),
                _ => continue,
            }
        }
    }

    pub fn call(&mut self, method: &str, params: Value, session: Option<&str>) -> Res<Value> {
        self.next += 1;
        let mut msg = json!({"id": self.next, "method": method, "params": params});
        if let Some(s) = session {
            msg["sessionId"] = json!(s);
        }
        self.ws.send(Message::Text(msg.to_string().into()))?;
        loop {
            let m = self.read()?;
            if m["id"] == self.next {
                return match m.get("error") {
                    Some(e) => Err(anyhow!("{method}: {e}")),
                    None => Ok(m["result"].clone()),
                };
            }
            if m.get("method").is_some() {
                self.events.push(m);
            }
        }
    }

    fn wait_event(&mut self, method: &str, session: &str) -> Res<()> {
        let hit = |e: &Value| e["method"] == method && e["sessionId"] == session;
        if let Some(i) = self.events.iter().position(hit) {
            self.events.drain(..=i);
            return Ok(());
        }
        loop {
            if hit(&self.read()?) {
                return Ok(());
            }
        }
    }

    /// The browser's cookies for `domain` and its subdomains, in `CookieParam` shape.
    pub fn cookies(&mut self, domain: &str) -> Res<Vec<Value>> {
        let all = self.call("Storage.getCookies", json!({}), None)?;
        Ok(all["cookies"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|c| c["domain"].as_str().is_some_and(|d| d.ends_with(domain)))
            .map(|c| {
                let mut p = json!({});
                for k in ["name", "value", "domain", "path", "secure", "httpOnly", "sameSite"] {
                    if let Some(v) = c.get(k) {
                        p[k] = v.clone();
                    }
                }
                if c["expires"].as_f64().is_some_and(|e| e > 0.0) {
                    p["expires"] = c["expires"].clone();
                }
                p
            })
            .collect())
    }
}

pub struct Response {
    pub status: u16,
    pub content_type: String,
    pub text: String,
}

pub struct Browser {
    cdp: Cdp,
    child: Child,
    /// One parked page per origin: an in-page fetch is subject to CORS.
    pages: HashMap<String, String>,
}

impl Browser {
    // ponytail: the profile is single-instance, so two concurrent CLI calls collide.
    pub fn launch(profile: &Path, cookies: &[Value], headless: bool) -> Res<Browser> {
        std::fs::create_dir_all(profile)?;
        let port_file = profile.join("DevToolsActivePort");
        let _ = std::fs::remove_file(&port_file);
        let mut child = spawn_chrome(profile, 0, headless, "about:blank")?;

        let deadline = Instant::now() + Duration::from_secs(20);
        let ws_url = loop {
            if let Some((port, path)) = std::fs::read_to_string(&port_file)
                .ok()
                .and_then(|s| s.trim().split_once('\n').map(|(a, b)| (a.to_string(), b.to_string())))
            {
                break format!("ws://127.0.0.1:{port}{path}");
            }
            if Instant::now() > deadline || child.try_wait().ok().flatten().is_some() {
                let _ = child.kill();
                bail!("Chrome did not start (is another courses command running for this provider?)");
            }
            std::thread::sleep(Duration::from_millis(100));
        };

        let mut cdp = Cdp::connect(&ws_url)?;
        if !cookies.is_empty() {
            cdp.call("Storage.setCookies", json!({"cookies": cookies}), None)?;
        }
        Ok(Browser { cdp, child, pages: HashMap::new() })
    }

    fn new_page(&mut self) -> Res<String> {
        let t = self.cdp.call("Target.createTarget", json!({"url": "about:blank"}), None)?;
        let a = self.cdp.call("Target.attachToTarget", json!({"targetId": t["targetId"], "flatten": true}), None)?;
        let sid = a["sessionId"].as_str().ok_or_else(|| anyhow!("no sessionId"))?.to_string();
        self.cdp.call("Page.enable", json!({}), Some(&sid))?;
        Ok(sid)
    }

    fn eval(&mut self, sid: &str, expr: &str) -> Res<Value> {
        let r = self.cdp.call(
            "Runtime.evaluate",
            json!({"expression": expr, "awaitPromise": true, "returnByValue": true}),
            Some(sid),
        )?;
        if let Some(ex) = r.get("exceptionDetails") {
            bail!("{}", ex["exception"]["description"].as_str().unwrap_or("JS exception"));
        }
        Ok(r["result"]["value"].clone())
    }

    /// Navigate and return the final URL once the HTTP redirect chain settled.
    fn goto(&mut self, sid: &str, url: &str) -> Res<String> {
        self.cdp.events.retain(|e| e["sessionId"] != sid);
        let r = self.cdp.call("Page.navigate", json!({"url": url}), Some(sid))?;
        if let Some(err) = r["errorText"].as_str() {
            bail!("{url}: {err}");
        }
        self.cdp.wait_event("Page.domContentEventFired", sid)?;
        Ok(self.eval(sid, "location.href")?.as_str().unwrap_or("").into())
    }

    /// Visit `url` in a throwaway page; Chrome stores the cookies itself.
    pub fn visit(&mut self, url: &str) -> Res<String> {
        let sid = self.new_page()?;
        let out = self.goto(&sid, url);
        let _ = self.cdp.call("Page.close", json!({}), Some(&sid));
        out
    }

    /// Open `url` in a tab left open, for a human to use (visible browser).
    pub fn open(&mut self, url: &str) -> Res<()> {
        let sid = self.new_page()?;
        self.goto(&sid, url).map(|_| ())
    }

    fn page_for(&mut self, origin: &str, park: &str) -> Res<String> {
        if let Some(sid) = self.pages.get(origin) {
            return Ok(sid.clone());
        }
        let sid = self.new_page()?;
        let _ = self.goto(&sid, park);
        self.pages.insert(origin.into(), sid.clone());
        Ok(sid)
    }

    pub fn fetch(&mut self, req: &crate::spec::Request) -> Res<Response> {
        let origin = url::Url::parse(&req.url)?.origin().ascii_serialization();
        // The IAM root aborts; its login UI loads. A 404 elsewhere is fine: we only need the origin.
        let park = if origin == crate::session::IAM_ORIGIN { "/iam/XUI/" } else { "/" };
        self.fetch_from(&format!("{origin}{park}"), req)
    }

    /// `fetch` from a page parked on `page`, for sites whose bot check needs
    /// its script to have run on a real HTML page (DataDome). `page` must be
    /// same-origin with the request, or allowed by its CORS policy.
    pub fn fetch_from(&mut self, page: &str, req: &crate::spec::Request) -> Res<Response> {
        let origin = url::Url::parse(page)?.origin().ascii_serialization();
        let mut payload = json!({"method": req.method, "url": req.url, "headers": req.headers});
        payload["headers"]["accept"] = json!(req.headers.get("accept").and_then(Value::as_str).unwrap_or("application/json, text/plain, */*"));
        match &req.body {
            Some(crate::spec::Body::Text(t)) => payload["body"] = json!(t),
            Some(crate::spec::Body::Form(f)) => payload["form"] = json!(f),
            None => {}
        }
        let expr = format!(
            "(async (req) => {{
                let body = req.body;
                if (req.form) {{ body = new FormData(); for (const [k, v] of req.form) body.append(k, v); }}
                const r = await fetch(req.url, {{ method: req.method, headers: req.headers, body, credentials: 'include' }});
                return {{ status: r.status, contentType: r.headers.get('content-type') || '', text: await r.text() }};
            }})({payload})"
        );
        let run = |b: &mut Browser| -> Res<Response> {
            let sid = b.page_for(&origin, page)?;
            let v = b.eval(&sid, &expr)?;
            Ok(Response {
                status: v["status"].as_u64().unwrap_or(0) as u16,
                content_type: v["contentType"].as_str().unwrap_or("").into(),
                text: v["text"].as_str().unwrap_or("").into(),
            })
        };
        run(self).or_else(|_| {
            // A navigation can tear the page's context down mid-call; re-park once.
            if let Some(sid) = self.pages.remove(&origin) {
                let _ = self.cdp.call("Page.close", json!({}), Some(&sid));
            }
            run(self)
        })
    }

    /// Evaluate `expr` in the page parked on `page` (as `fetch_from` parks it),
    /// e.g. to read a config the site's own scripts set.
    pub fn eval_in(&mut self, page: &str, expr: &str) -> Res<Value> {
        let origin = url::Url::parse(page)?.origin().ascii_serialization();
        let sid = self.page_for(&origin, page)?;
        self.eval(&sid, expr)
    }

    pub fn cookies(&mut self, domain: &str) -> Res<Vec<Value>> {
        self.cdp.cookies(domain)
    }

    /// Set cookies (CDP `CookieParam` objects), e.g. ones a site's own JS would set.
    pub fn set_cookies(&mut self, cookies: Value) -> Res<()> {
        self.cdp.call("Storage.setCookies", json!({ "cookies": cookies }), None).map(|_| ())
    }
}

impl Drop for Browser {
    fn drop(&mut self) {
        // A clean close lets Chrome flush the profile.
        let _ = self.cdp.call("Browser.close", json!({}), None);
        for _ in 0..50 {
            if self.child.try_wait().ok().flatten().is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

