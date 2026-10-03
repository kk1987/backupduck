use backupduck_cloud_audit::{rpc::*, Error};
use serde_json::json;

fn envelope(rpcid: &str, inner: &str) -> String {
    let line = json!([
        ["wrb.fr", rpcid, inner, null, null, null, "generic"],
        ["di", 120],
        ["af.httprm", 119, "-123", 4]
    ])
    .to_string();
    format!(
        ")]}}'\n\n{}\n{line}\n25\n[[\"e\",4,null,null,{}]]\n",
        line.len(),
        line.len()
    )
}

#[test]
fn extracts_inner_payload() {
    let inner = json!([[["hash=", ["key", null]]]]).to_string();
    let body = envelope("swbisb", &inner);
    assert_eq!(
        parse_envelope(&body, "swbisb").unwrap(),
        json!([[["hash=", ["key", null]]]])
    );
}

#[test]
fn literal_wire_text() {
    let body = ")]}'\n\n123\n[[\"wrb.fr\",\"swbisb\",\"[[[\\\"aGFzaA==\\\",[\\\"AF1Qip\\\"]]]]\",null,null,null,\"generic\"]]\n";
    assert_eq!(
        parse_envelope(body, "swbisb").unwrap(),
        json!([[["aGFzaA==", ["AF1Qip"]]]])
    );
}

#[test]
fn errors() {
    // An empty payload string is how batchexecute reports an RPC failure.
    assert!(matches!(
        parse_envelope(&envelope("swbisb", ""), "swbisb"),
        Err(Error::Parse(_))
    ));
    let null_payload = ")]}'\n[[\"wrb.fr\",\"swbisb\",null,null,null,[3],\"generic\"]]\n";
    assert!(matches!(
        parse_envelope(null_payload, "swbisb"),
        Err(Error::Parse(_))
    ));
    assert!(matches!(parse_envelope("", "swbisb"), Err(Error::Parse(_))));
    assert!(matches!(
        parse_envelope(")]}'\n\n", "swbisb"),
        Err(Error::Parse(_))
    ));
    assert!(matches!(
        parse_envelope(&envelope("VrseUb", "[]"), "swbisb"),
        Err(Error::Parse(_))
    ));
    assert!(matches!(
        parse_envelope(&envelope("swbisb", "{not json"), "swbisb"),
        Err(Error::Parse(_))
    ));
}

#[test]
fn request_form() {
    let body = request_body("swbisb", &json!([["aGFzaA=="], null, 3, 0]), "tok:1");
    let pairs: Vec<(String, String)> = url::form_urlencoded::parse(body.as_bytes())
        .into_owned()
        .collect();
    assert_eq!(pairs[0].0, "f.req");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&pairs[0].1).unwrap(),
        json!([[["swbisb", "[[\"aGFzaA==\"],null,3,0]", null, "generic"]]])
    );
    assert_eq!(pairs[1], ("at".into(), "tok:1".into()));
}
