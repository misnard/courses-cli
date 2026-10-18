//! Carrefour session: cookie jar, SSO renewal and interactive login.
//! Flow from https://github.com/maximeallanic/CarrefourDriveMCP (MIT).
//!
//! Logging in ends with two sessions: the ForgeRock SSO cookie `c4iamsecuretk`
//! on moncompte.carrefour.fr (24 h max, dies after 60 min idle) and short-lived
//! storefront cookies on www.carrefour.fr. Only the first needs a human
//! (Turnstile captcha + OTP); the second is rebuilt from it by replaying the
//! OAuth2 redirect chain.

use crate::browser::{devtools_get, spawn_chrome, Browser, Cdp, Response};
use crate::config::config_dir;
use crate::spec::Request;
use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub const CARREFOUR_ORIGIN: &str = "https://www.carrefour.fr";
pub const IAM_ORIGIN: &str = "https://moncompte.carrefour.fr";
pub const SSO_COOKIE: &str = "c4iamsecuretk";
pub const NEED_LOGIN: &str = "run 'carrefour auth login' to sign in (captcha + OTP)";

// ---------------------------------------------------------------- cookie jar
// The jar holds CDP `CookieParam` objects. Chrome never writes session cookies
// (like the SSO one) to disk, so we persist them ourselves and re-inject them
// at every launch.

pub fn jar_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("cookies.json"))
}

pub fn load_jar() -> Result<Vec<Value>> {
    let mut jar: Vec<Value> = std::fs::read(jar_path()?)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    if let Ok(env) = std::env::var("CARREFOUR_COOKIES") {
        merge(&mut jar, parse_cookies(&env));
    }
    Ok(jar)
}

pub fn save_jar(jar: &[Value]) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let path = jar_path()?;
    std::fs::create_dir_all(path.parent().unwrap())?;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&path)
        .with_context(|| format!("Failed to write {:?}", path))?;
    f.write_all(serde_json::to_string_pretty(jar)?.as_bytes())?;
    Ok(())
}

pub fn merge(jar: &mut Vec<Value>, new: Vec<Value>) {
    let key = |c: &Value| {
        let domain = c["domain"].as_str().unwrap_or("").trim_start_matches('.').to_string();
        (c["name"].clone(), domain, c["path"].clone())
    };
    for c in new {
        jar.retain(|o| key(o) != key(&c));
        jar.push(c);
    }
}

pub fn has_sso(jar: &[Value]) -> bool {
    jar.iter().any(|c| c["name"] == SSO_COOKIE)
}

pub fn storefront_cookies(jar: &[Value]) -> usize {
    jar.iter()
        .filter(|c| c["domain"].as_str().is_some_and(|d| d.ends_with("www.carrefour.fr")))
        .count()
}

/// Accepts a `Cookie:` header, a JSON `{name: value}` map, or a JSON array
/// (DevTools / Playwright / EditThisCookie export — the only shape carrying
/// the domain, hence the only one that can supply the SSO cookie).
pub fn parse_cookies(input: &str) -> Vec<Value> {
    let cookie = |name: &str, value: &str| {
        json!({"name": name, "value": value, "domain": "www.carrefour.fr", "path": "/", "secure": true})
    };
    match serde_json::from_str::<Value>(input.trim()) {
        Ok(Value::Array(items)) => items
            .iter()
            .filter(|c| c["name"].is_string())
            .map(|c| {
                let mut p = cookie(c["name"].as_str().unwrap(), c["value"].as_str().unwrap_or(""));
                for k in ["domain", "path", "secure", "httpOnly"] {
                    if !c[k].is_null() {
                        p[k] = c[k].clone();
                    }
                }
                let expires = c["expires"].as_f64().or(c["expirationDate"].as_f64());
                if let Some(e) = expires.filter(|e| *e > 0.0) {
                    p["expires"] = json!(e);
                }
                p
            })
            .collect(),
        Ok(Value::Object(map)) => map
            .iter()
            .map(|(k, v)| cookie(k, v.as_str().unwrap_or(&v.to_string())))
            .collect(),
        _ => input
            .split(';')
            .filter_map(|p| p.split_once('='))
            .filter(|(k, _)| !k.trim().is_empty())
            .map(|(k, v)| cookie(k.trim(), v.trim()))
            .collect(),
    }
}

// ------------------------------------------------------------------- session

pub struct Session {
    pub browser: Browser,
    sso: bool,
    storefront: bool,
}

impl Session {
    pub fn open() -> Result<Session> {
        let jar = load_jar()?;
        Ok(Session {
            sso: has_sso(&jar),
            storefront: storefront_cookies(&jar) > 0,
            browser: Browser::launch(&config_dir()?.join("browser-profile"), &jar)?,
        })
    }

    /// Replay authorize → /login/check: while the SSO cookie lives, Carrefour
    /// mints fresh storefront cookies, and Chrome stores them itself.
    pub fn refresh(&mut self) -> Result<()> {
        if !self.sso {
            bail!("No SSO cookie ({SSO_COOKIE}) stored; {NEED_LOGIN}.");
        }
        let authorize = url::Url::parse_with_params(
            &format!("{IAM_ORIGIN}/iam/oauth2/CarrefourConnect/authorize"),
            [
                ("client_id", "carrefour_onecarrefour_web"),
                ("redirect_uri", &format!("{CARREFOUR_ORIGIN}/login/check")),
                ("response_type", "code"),
                ("scope", "openid iam"),
            ],
        )?;
        let landed = self.browser.visit(authorize.as_str())?;
        if !landed.starts_with(CARREFOUR_ORIGIN) {
            bail!("The SSO session has expired (24 h max, 60 min idle); {NEED_LOGIN}.");
        }
        self.storefront = true;
        Ok(())
    }

    /// Send a request, renewing the storefront session once on 401/403.
    pub fn send(&mut self, req: &Request) -> Result<Response> {
        if self.sso && !self.storefront {
            self.refresh()?;
        }
        let first = self.browser.fetch(req)?;
        if !self.sso || !matches!(first.status, 401 | 403) {
            return Ok(first);
        }
        match self.refresh() {
            Ok(()) => self.browser.fetch(req),
            Err(e) => {
                eprintln!("{e}");
                Ok(first)
            }
        }
    }
}

impl Drop for Session {
    /// Persist the browser's cookies, renewed ones included.
    fn drop(&mut self) {
        match self.browser.cookies() {
            Ok(c) if !c.is_empty() => {
                if let Err(e) = save_jar(&c) {
                    eprintln!("Warning: could not save cookies: {e}");
                }
            }
            _ => {}
        }
    }
}

// --------------------------------------------------------------------- login

/// Turnstile refuses to validate in a CDP-driven browser, so the login window
/// is a plain Chrome: we poll its tab list over HTTP and only attach once the
/// OAuth loop landed back on the storefront, to lift the cookies from memory.
pub fn login() -> Result<Vec<Value>> {
    let port = std::net::TcpListener::bind("127.0.0.1:0")?.local_addr()?.port();
    let dir = std::env::temp_dir().join(format!("carrefour-login-{}", std::process::id()));
    let mut child = spawn_chrome(&dir, port, false, &format!("{CARREFOUR_ORIGIN}/mon-compte/login"))?;

    let signed_in = |url: &str| {
        url.strip_prefix(CARREFOUR_ORIGIN)
            .is_some_and(|r| r.starts_with('/') && !r.starts_with("/mon-compte/login"))
    };
    let deadline = Instant::now() + Duration::from_secs(900);
    let mut done = false;
    while !done && Instant::now() < deadline {
        std::thread::sleep(Duration::from_secs(2));
        if child.try_wait().ok().flatten().is_some() {
            break;
        }
        let Ok(tabs) = devtools_get(port, "/json/list") else { continue };
        let pages: Vec<&Value> = tabs.as_array().into_iter().flatten().filter(|t| t["type"] == "page").collect();
        if pages.is_empty() {
            break; // window closed
        }
        done = pages.iter().any(|t| signed_in(t["url"].as_str().unwrap_or("")));
    }

    let cookies = if done {
        devtools_get(port, "/json/version")
            .and_then(|v| Cdp::connect(v["webSocketDebuggerUrl"].as_str().unwrap_or(""))?.cookies())
    } else {
        Err(anyhow!("The login did not complete. Run it again and leave the window open once signed in."))
    };
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&dir);

    let cookies = cookies?;
    save_jar(&cookies)?;
    Ok(cookies)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_merges_cookies() {
        assert_eq!(parse_cookies("a=1; b=x=y")[1]["value"], "x=y");
        assert_eq!(parse_cookies(r#"{"a":"1"}"#)[0]["domain"], "www.carrefour.fr");
        let arr = parse_cookies(r#"[{"name":"c4iamsecuretk","value":"t","domain":".moncompte.carrefour.fr","expires":-1}]"#);
        assert!(has_sso(&arr) && arr[0]["expires"].is_null());

        let mut jar = parse_cookies("a=1");
        merge(&mut jar, parse_cookies("a=2; b=3"));
        assert_eq!(jar.len(), 2);
        assert_eq!(jar.iter().find(|c| c["name"] == "a").unwrap()["value"], "2");
    }
}
