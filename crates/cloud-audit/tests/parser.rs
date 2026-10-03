use backupduck_cloud_audit::{parser::*, Error};
use serde_json::{json, Value};

fn fixture(name: &str) -> Value {
    let path = format!(
        "{}/tests/fixtures/parser/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn swbisb_fixture() {
    let matches = parse_hash_matches(&fixture("swbisb")).unwrap();
    assert_eq!(matches.len(), 4);
    let first = &matches[0];
    assert_eq!(first.hash_b64, "bOmN91IO001JMzaKS96XuMu6yT8=");
    assert_eq!(
        first.media_key,
        "AF1QipcCV2peehAtAHwS1JC8ChoTcqk1qiR7vRZIZVqq"
    );
    assert_eq!(
        first.dedup_key.as_deref(),
        Some("MQDzsaTM-B5FwE2Cu2udSXvWZOb2")
    );
    assert_eq!(first.device_model.as_deref(), Some("SM-G980F"));
    assert_eq!((first.width, first.height), (Some(2159), Some(1619)));
    assert_eq!(first.timestamp_ms, Some(1657794136000));
    assert_eq!(first.creation_timestamp_ms, Some(1769780017999));
    assert_eq!(matches[1].hash_b64, "y/Ilyr5+s8tJlOvDERvQDbVRG9o=");
    assert_eq!(
        (matches[1].width, matches[1].height),
        (Some(1413), Some(1883))
    );
    assert!(matches
        .iter()
        .all(|m| m.device_model.as_deref() == Some("SM-G980F")));
}

#[test]
fn ewgk9e_fixture() {
    // The fixture is Toolkit's already-sliced `response[0][1]`.
    let items = fixture("EWgK9e");
    let infos = parse_batch_items(&items).unwrap();
    assert_eq!(infos.len(), 16);
    let first = &infos[0];
    assert_eq!(
        first.media_key,
        "AF1QipUTORt0UuUoEnf3LUfylqNvwB42tg6aR7tBD7nY"
    );
    assert_eq!(first.file_name.as_deref(), Some("file_164.mp4"));
    assert_eq!(first.size, Some(39680079));
    // Quota vector [2, 0, 2, 0, null, 1].
    assert_eq!(first.takes_up_space, Some(false));
    assert_eq!(first.space_taken, Some(0));
    assert_eq!(first.is_original_quality, Some(true));
    assert_eq!(infos[2].file_name.as_deref(), Some("file_166.jpg"));
    assert_eq!(infos[2].size, Some(3722321));
    assert!(infos.iter().all(|i| i.takes_up_space == Some(false)));
    // Same items through the full-response entry point.
    assert_eq!(parse_batch_info(&json!([[null, items]])).unwrap(), infos);
}

#[test]
fn vrseub_fixture() {
    let info = parse_item_info(&fixture("VrseUb")).unwrap();
    assert_eq!(
        info.media_key,
        "AF1Qip2mTIZGakCgGqW-3V0rvMUJ6FAqKuPZZmupWZRs"
    );
    // ext["318563170"][0] = [2, 0, 2, 0, null, 1]; VrseUb has no name or size.
    assert_eq!(info.takes_up_space, Some(false));
    assert_eq!(info.space_taken, Some(0));
    assert_eq!(info.is_original_quality, Some(true));
    assert_eq!(info.file_name, None);
    assert_eq!(info.size, None);
}

#[test]
fn fdcn4b_fixture() {
    let info = parse_item_info_ext(&fixture("fDcn4b")).unwrap();
    assert_eq!(
        info.media_key,
        "AF1Qip2mTIZGakCgGqW-3V0rvMUJ6FAqKuPZZmupWZRs"
    );
    assert_eq!(info.file_name.as_deref(), Some("file_286.mov"));
    assert_eq!(info.size, Some(908786));
    assert_eq!(info.takes_up_space, Some(false));
    assert_eq!(info.space_taken, Some(0));
    assert_eq!(info.is_original_quality, Some(true));
}

#[test]
fn quota_flag_counts() {
    let items = json!([[
        "k",
        [
            null,
            null,
            "",
            "a.jpg",
            null,
            null,
            1,
            2,
            3,
            10,
            [1, 5120, 1]
        ]
    ]]);
    let info = &parse_batch_items(&items).unwrap()[0];
    assert_eq!(info.takes_up_space, Some(true));
    assert_eq!(info.space_taken, Some(5120));
    assert_eq!(info.is_original_quality, Some(false));
}

#[test]
fn empty_results() {
    assert!(parse_hash_matches(&json!([])).unwrap().is_empty());
    assert!(parse_hash_matches(&json!([null])).unwrap().is_empty());
    assert!(parse_hash_matches(&json!([[]])).unwrap().is_empty());
    assert!(parse_batch_info(&json!([])).unwrap().is_empty());
    assert!(parse_batch_info(&json!([[null, null]])).unwrap().is_empty());
}

#[test]
fn garbled_values_never_panic() {
    // Wrong top-level shapes are errors.
    for bad in [
        json!(null),
        json!("x"),
        json!({"a": 1}),
        json!(["not a list"]),
    ] {
        assert!(
            matches!(parse_hash_matches(&bad), Err(Error::Parse(_))),
            "{bad}"
        );
    }
    for bad in [json!(null), json!(7), json!([[null, "x"]])] {
        assert!(
            matches!(parse_batch_info(&bad), Err(Error::Parse(_))),
            "{bad}"
        );
    }
    assert!(matches!(
        parse_batch_items(&json!({})),
        Err(Error::Parse(_))
    ));
    for bad in [
        json!(null),
        json!([]),
        json!([[]]),
        json!([[null]]),
        json!([{}]),
    ] {
        assert!(
            matches!(parse_item_info(&bad), Err(Error::Parse(_))),
            "{bad}"
        );
        assert!(
            matches!(parse_item_info_ext(&bad), Err(Error::Parse(_))),
            "{bad}"
        );
    }

    // Uncorrelatable entries are dropped; mistyped fields become None.
    let matches = parse_hash_matches(&json!([[
        null,
        "x",
        [],
        ["only-hash"],
        ["h", []],
        ["h", [7]],
        ["h", ["k", "thumb", "ts", 3, null, true]],
        [
            "h2",
            [
                "k2",
                [
                    null,
                    -1,
                    1.5,
                    null,
                    null,
                    null,
                    null,
                    null,
                    [1, 2, 3, 4, "model"]
                ]
            ]
        ],
        [
            "h3",
            [
                "k3",
                [
                    null,
                    1,
                    1,
                    null,
                    null,
                    null,
                    null,
                    null,
                    [1, 2, 3, 4, [null, 9]]
                ]
            ]
        ]
    ]]))
    .unwrap();
    assert_eq!(matches.len(), 3);
    assert_eq!(matches[0].media_key, "k");
    assert_eq!(matches[0].width, None);
    assert_eq!(matches[0].timestamp_ms, None);
    assert_eq!(matches[0].dedup_key, None);
    assert_eq!(matches[1].width, None);
    assert_eq!(matches[1].height, None);
    assert_eq!(matches[1].device_model, None);
    assert_eq!(matches[2].device_model, None);

    let infos = parse_batch_items(&json!([
        null,
        ["k"],
        ["k2", "detail"],
        [
            "k3",
            [
                null,
                null,
                null,
                5,
                null,
                null,
                null,
                null,
                null,
                "big",
                ["1", -1, null]
            ]
        ],
        [
            "k4",
            [null, null, null, null, null, null, null, null, null, null, "tail"]
        ]
    ]))
    .unwrap();
    assert_eq!(infos.len(), 4);
    assert!(infos.iter().all(|i| i.file_name.is_none()
        && i.size.is_none()
        && i.takes_up_space.is_none()
        && i.space_taken.is_none()
        && i.is_original_quality.is_none()));

    let info = parse_item_info(&json!([["k", null, {"318563170": "bad"}]])).unwrap();
    assert_eq!(info.takes_up_space, None);
    let info = parse_item_info_ext(&json!([["k", null, 3, null, null, "big"]])).unwrap();
    assert_eq!(
        (info.file_name, info.size, info.takes_up_space),
        (None, None, None)
    );
}
