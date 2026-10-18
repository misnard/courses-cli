//! Turns a `tools/*.json` spec plus call arguments into a concrete request.
//! Port of CarrefourDriveMCP's `src/spec/resolve.ts`. Pure, no network.

use serde_json::{Map, Value, json};

pub enum Body {
    Text(String),
    /// multipart/form-data, rebuilt as a FormData inside the page.
    Form(Vec<(String, String)>),
}

pub struct Request {
    pub method: String,
    pub url: String,
    pub headers: Map<String, Value>,
    pub body: Option<Body>,
}

fn param_ref(v: &Value) -> Option<&str> {
    v.as_object().filter(|o| o.len() == 1)?.get("$param")?.as_str()
}

/// Replace `{"$param": "x"}` nodes; placeholders without an argument vanish (None).
pub fn substitute(t: &Value, args: &Map<String, Value>) -> Option<Value> {
    if let Some(name) = param_ref(t) {
        return args.get(name).cloned();
    }
    Some(match t {
        Value::Array(a) => Value::Array(a.iter().filter_map(|x| substitute(x, args)).collect()),
        Value::Object(o) => Value::Object(
            o.iter()
                .filter_map(|(k, v)| substitute(v, args).map(|r| (k.clone(), r)))
                .collect(),
        ),
        _ => t.clone(),
    })
}

fn scalar(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Key/value pairs from a template map; arrays become repeated keys.
fn pairs(map: Option<&Value>, args: &Map<String, Value>) -> Vec<(String, String)> {
    let mut out = vec![];
    for (k, t) in map.and_then(Value::as_object).into_iter().flatten() {
        match substitute(t, args) {
            None | Some(Value::Null) => {}
            Some(Value::Array(items)) => {
                out.extend(items.iter().filter(|i| !i.is_null()).map(|i| (k.clone(), scalar(i))))
            }
            Some(v) => out.push((k.clone(), scalar(&v))),
        }
    }
    out
}

fn encode_component(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Fill `{segment}` placeholders in the URL path.
pub fn substitute_url(url: &str, args: &Map<String, Value>) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = url;
    while let Some(start) = rest.find('{') {
        let end = rest[start..].find('}').ok_or("unterminated {segment}")? + start;
        let name = &rest[start + 1..end];
        let value = match args.get(name) {
            None | Some(Value::Null) => None,
            Some(v) => Some(scalar(v)).filter(|s| !s.is_empty()),
        }
        .ok_or_else(|| format!("missing required URL path parameter \"{name}\""))?;
        out.push_str(&rest[..start]);
        out.push_str(&encode_component(&value));
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

pub fn build(spec: &Value, args: &Map<String, Value>) -> Result<Request, String> {
    let req = &spec["request"];
    let method = req["method"].as_str().unwrap_or("GET").to_uppercase();
    let mut url = url::Url::parse(&substitute_url(req["url"].as_str().unwrap_or(""), args)?)
        .map_err(|e| e.to_string())?;
    for (k, v) in pairs(req.get("query"), args) {
        url.query_pairs_mut().append_pair(&k, &v);
    }
    let mut headers: Map<String, Value> = pairs(req.get("headers"), args)
        .into_iter()
        .map(|(k, v)| (k.to_lowercase(), json!(v)))
        .collect();

    let resolved = match req.get("body") {
        _ if method == "GET" || method == "HEAD" => None,
        Some(b) if !b.is_null() => substitute(b, args),
        _ => None,
    };
    let content_type = req["content_type"].as_str().unwrap_or("application/json");
    let body = resolved.map(|b| {
        if content_type.starts_with("application/x-www-form-urlencoded") {
            let mut s = url::form_urlencoded::Serializer::new(String::new());
            for (k, v) in pairs(Some(&b), &Map::new()) {
                s.append_pair(&k, &v);
            }
            (Body::Text(s.finish()), Some("application/x-www-form-urlencoded"))
        } else if content_type.starts_with("multipart/form-data") {
            // fetch generates the boundary, so the header must not be pinned.
            (Body::Form(pairs(Some(&b), &Map::new())), None)
        } else {
            (Body::Text(b.to_string()), Some("application/json"))
        }
    });

    headers.remove("content-type");
    if let Some((_, Some(ct))) = &body {
        headers.insert("content-type".into(), json!(ct));
    }
    Ok(Request { method, url: url.into(), headers, body: body.map(|(b, _)| b) })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn builds_requests() {
        let spec = json!({"request": {
            "method": "POST",
            "url": "https://x.fr/api/{store_id}/items",
            "headers": {"Content-Type": "application/json", "X-Opt": {"$param": "opt"}},
            "query": {"codes[]": {"$param": "codes"}, "page": {"$param": "page"}},
            "body": {"items": [{"ean": {"$param": "ean"}, "qty": {"$param": "qty"}}], "missing": {"$param": "nope"}},
        }});
        let r = build(&spec, &args(json!({"store_id": "a b", "codes": [14, 15], "ean": "123", "qty": 2}))).unwrap();
        assert_eq!(r.url, "https://x.fr/api/a%20b/items?codes%5B%5D=14&codes%5B%5D=15");
        assert_eq!(r.headers.len(), 1);
        assert!(matches!(r.body, Some(Body::Text(ref b)) if b == r#"{"items":[{"ean":"123","qty":2}]}"#));

        assert!(build(&spec, &args(json!({}))).err().unwrap().contains("store_id"));

        let get = json!({"request": {"method": "GET", "url": "https://x.fr/s", "headers": {"content-type": "application/json"}, "body": {"a": 1}}});
        let r = build(&get, &Map::new()).unwrap();
        assert!(r.body.is_none() && r.headers.is_empty());

        let form = json!({"request": {"method": "POST", "url": "https://x.fr/", "content_type": "application/x-www-form-urlencoded", "body": {"title": {"$param": "t"}}}});
        let r = build(&form, &args(json!({"t": "ma liste"}))).unwrap();
        assert!(matches!(r.body, Some(Body::Text(ref b)) if b == "title=ma+liste"));
    }
}
