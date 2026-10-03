//! `batchexecute` request/response framing.

use crate::{
    session::{check_status, Session, ORIGIN},
    Error, Result,
};
use serde_json::{json, Value};
use std::time::Duration;

/// Toolkit retries after `2 s * attempt`; we retry once.
const RETRY_DELAY: Duration = Duration::from_secs(2);

#[derive(Debug)]
enum Attempt {
    /// Empty body or no `wrb.fr` line: worth one retry.
    Retry(Error),
    Fatal(Error),
}

/// Extracts the payload for `rpcid` from a `batchexecute` response body.
///
/// The body starts with `)]}'`, followed by newline-separated chunks, some of
/// which are decimal lengths. Lines containing `"wrb.fr"` are JSON arrays of
/// envelopes `["wrb.fr", rpcid, "<payload JSON string>", .., payload id @6]`.
/// An empty payload string signals an RPC error.
pub fn parse_envelope(body: &str, rpcid: &str) -> Result<Value> {
    parse(body, rpcid).map_err(|a| match a {
        Attempt::Retry(e) | Attempt::Fatal(e) => e,
    })
}

fn parse(body: &str, rpcid: &str) -> std::result::Result<Value, Attempt> {
    let body = body.trim_start();
    let body = body.strip_prefix(")]}'").unwrap_or(body);
    if body.trim().is_empty() {
        return Err(Attempt::Retry(Error::Parse("empty response body".into())));
    }
    let mut seen = false;
    for line in body.lines().filter(|l| l.contains("\"wrb.fr\"")) {
        seen = true;
        let Ok(Value::Array(envelopes)) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        for envelope in &envelopes {
            if envelope.get(0).and_then(Value::as_str) != Some("wrb.fr")
                || envelope.get(1).and_then(Value::as_str) != Some(rpcid)
            {
                continue;
            }
            return match envelope.get(2).and_then(Value::as_str) {
                Some(payload) if !payload.is_empty() => {
                    serde_json::from_str(payload).map_err(|_| {
                        Attempt::Fatal(Error::Parse(format!("{rpcid} payload is not JSON")))
                    })
                }
                _ => Err(Attempt::Fatal(Error::Parse(format!(
                    "{rpcid} returned an empty payload"
                )))),
            };
        }
    }
    let message = if seen {
        format!("no {rpcid} envelope in response")
    } else {
        "no wrb.fr envelope in response".into()
    };
    Err(Attempt::Retry(Error::Parse(message)))
}

/// Form body: `f.req=[[[rpcid, "<payload JSON>", null, "generic"]]]&at=<xsrf>`.
pub fn request_body(rpcid: &str, payload: &Value, at: &str) -> String {
    let inner = json!([[[rpcid, payload.to_string(), null, "generic"]]]);
    url::form_urlencoded::Serializer::new(String::new())
        .append_pair("f.req", &inner.to_string())
        .append_pair("at", at)
        .finish()
}

impl Session {
    /// Sends one RPC and returns its unwrapped payload. Retries once, after
    /// 2 s, when the body is empty or carries no envelope.
    pub async fn batchexecute(&self, rpcid: &str, payload: &Value) -> Result<Value> {
        match self.attempt(rpcid, payload).await {
            Err(Attempt::Retry(_)) => {
                tokio::time::sleep(RETRY_DELAY).await;
                self.attempt(rpcid, payload).await
            }
            other => other,
        }
        .map_err(|a| match a {
            Attempt::Retry(e) | Attempt::Fatal(e) => e,
        })
    }

    async fn attempt(&self, rpcid: &str, payload: &Value) -> std::result::Result<Value, Attempt> {
        let g = &self.globals;
        let url = format!("{ORIGIN}{}/data/batchexecute", g.rpc_path);
        let response = self
            .client
            .post(url)
            .query(&[
                ("rpcids", rpcid),
                ("source-path", self.base_path.as_str()),
                ("f.sid", g.f_sid.as_str()),
                ("bl", g.bl.as_str()),
                ("rt", "c"),
            ])
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded;charset=UTF-8",
            )
            .body(request_body(rpcid, payload, &g.at))
            .send()
            .await
            .map_err(|e| Attempt::Fatal(e.into()))?;
        check_status(response.status()).map_err(Attempt::Fatal)?;
        let body = response
            .text()
            .await
            .map_err(|e| Attempt::Fatal(e.into()))?;
        parse(&body, rpcid)
    }
}
