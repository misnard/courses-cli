//! Generic access to the 43 carrefour.fr endpoints described in `tools/*.json`
//! (from https://github.com/maximeallanic/CarrefourDriveMCP, MIT).

use crate::client::CarrefourClient;
use crate::spec;
use anyhow::{anyhow, bail, Context, Result};
use colored::Colorize;
use include_dir::{include_dir, Dir};
use serde_json::{json, Map, Value};
use std::io::Write;

static TOOLS: Dir = include_dir!("$CARGO_MANIFEST_DIR/tools");

fn tools() -> Vec<Value> {
    let mut specs: Vec<Value> = TOOLS
        .files()
        .filter(|f| f.path().extension().is_some_and(|e| e == "json"))
        .map(|f| serde_json::from_slice(f.contents()).expect("invalid embedded tool spec"))
        .collect();
    specs.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    specs
}

fn find(name: &str) -> Result<Value> {
    tools()
        .into_iter()
        .find(|t| t["name"] == name)
        .ok_or_else(|| anyhow!("Unknown endpoint '{}'. Run 'carrefour api' to list them.", name))
}

pub fn list() {
    for t in tools() {
        let auth = if t["requires_auth"] == true { "*".yellow() } else { " ".normal() };
        let url = t["request"]["url"].as_str().unwrap_or("").replace("https://www.carrefour.fr", "");
        println!(
            "{} {:<36} {:<6} {}",
            auth,
            t["name"].as_str().unwrap_or("").bold(),
            t["request"]["method"].as_str().unwrap_or(""),
            url.dimmed()
        );
    }
    println!("\n{}", "* needs login. 'carrefour api <name> --describe' for params.".dimmed());
}

pub fn describe(name: &str) -> Result<()> {
    let t = find(name)?;
    println!(
        "{} {} {}",
        name.bold(),
        t["request"]["method"].as_str().unwrap_or(""),
        t["request"]["url"].as_str().unwrap_or("")
    );
    println!("{}\n", t["description"].as_str().unwrap_or(""));
    let required = t["parameters"]["required"].as_array().cloned().unwrap_or_default();
    for (param, p) in t["parameters"]["properties"].as_object().into_iter().flatten() {
        let req = if required.contains(&json!(param)) { ", required" } else { "" };
        println!("  {} ({}{})", param.bold(), p["type"].as_str().unwrap_or("string"), req);
        println!("      {}", p["description"].as_str().unwrap_or("").dimmed());
    }
    Ok(())
}

fn coerce(raw: &str, p: &Value) -> Result<Value> {
    let ctx = || format!("'{}' is not a valid {}", raw, p["type"].as_str().unwrap_or("value"));
    Ok(match p["type"].as_str() {
        Some("number" | "integer") => Value::Number(raw.parse().with_context(ctx)?),
        Some("boolean") => Value::Bool(raw.parse().with_context(ctx)?),
        Some("object") => serde_json::from_str(raw).with_context(ctx)?,
        Some("array") => match serde_json::from_str::<Value>(raw) {
            Ok(v @ Value::Array(_)) => v,
            _ => Value::Array(raw.split(',').map(|s| coerce(s.trim(), &p["items"])).collect::<Result<_>>()?),
        },
        _ => json!(raw),
    })
}

pub fn call(name: &str, params: &[String], yes: bool) -> Result<()> {
    let spec = find(name)?;
    let props = &spec["parameters"]["properties"];
    let mut args = Map::new();
    for kv in params {
        let (k, v) = kv
            .split_once('=')
            .ok_or_else(|| anyhow!("Expected param=value, got '{}'", kv))?;
        let p = props
            .get(k)
            .ok_or_else(|| anyhow!("Unknown param '{}'. See 'carrefour api {} --describe'.", k, name))?;
        args.insert(k.to_string(), coerce(v, p)?);
    }
    let missing: Vec<&str> = spec["parameters"]["required"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|r| !args.contains_key(*r))
        .collect();
    if !missing.is_empty() {
        bail!("Missing param(s) {}. See 'carrefour api {} --describe'.", missing.join(", "), name);
    }
    if name == "submit_checkout_payment" && !yes {
        bail!("submit_checkout_payment charges a real payment. Re-run with --yes.");
    }

    let res = CarrefourClient::new()?.send(&spec::build(&spec, &args).map_err(|e| anyhow!(e))?)?;
    let out = match serde_json::from_str::<Value>(&res.text) {
        Ok(v) if res.content_type.contains("json") => serde_json::to_string_pretty(&v)?,
        _ => res.text,
    };
    // `| head` closing the pipe is not an error.
    let _ = writeln!(std::io::stdout(), "{}", out);
    if matches!(res.status, 401 | 403) {
        bail!("HTTP {}. {}.", res.status, crate::session::NEED_LOGIN);
    }
    if res.status >= 400 {
        bail!("HTTP {}", res.status);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coerces_and_embeds_specs() {
        assert_eq!(coerce("2", &json!({"type": "number"})).unwrap(), json!(2));
        assert_eq!(coerce("1,2", &json!({"type": "array", "items": {"type": "number"}})).unwrap(), json!([1, 2]));
        assert_eq!(coerce(r#"["a"]"#, &json!({"type": "array"})).unwrap(), json!(["a"]));
        assert!(coerce("x", &json!({"type": "boolean"})).is_err());
        assert_eq!(tools().len(), 43);
        for t in tools() {
            if let Err(e) = spec::build(&t, &Map::new()) {
                assert!(e.contains("URL path"), "{}: {}", t["name"], e);
            }
        }
    }
}
