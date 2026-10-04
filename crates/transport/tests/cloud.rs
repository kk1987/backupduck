use backupduck_core::*;
use backupduck_store::{retention::GalleryCopy, Receiver};
use backupduck_transport::Client;
use std::{
    collections::BTreeMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
};

const TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

struct Scratch(std::path::PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn receive(receiver: &mut Receiver, name: &str) -> String {
    let bytes = name.as_bytes();
    let asset = Asset {
        version: PROTOCOL_VERSION,
        source_id: name.into(),
        revision: "1".into(),
        kind: AssetKind::Photo,
        metadata: BTreeMap::new(),
        resources: vec![Resource {
            role: ResourceRole::Photo,
            filename: "IMG_0001.JPG".into(),
            media_type: "image/jpeg".into(),
            size: bytes.len() as u64,
            sha256: digest(bytes),
        }],
    };
    let id = receiver.register(asset).unwrap().asset_id;
    receiver
        .append(&id, &digest(bytes), 0, bytes, &digest(bytes))
        .unwrap();
    receiver.commit(&id).unwrap();
    id
}

#[tokio::test]
async fn auditor_lists_copies_and_reports_verdicts() {
    let root = Scratch(
        std::env::temp_dir().join(format!("backupduck-cloud-route-{}", std::process::id())),
    );
    std::fs::create_dir_all(&root.0).unwrap();
    let mut receiver = Receiver::open(&root.0, 1 << 20).unwrap();
    let id = receive(&mut receiver, "published");
    let legacy = receive(&mut receiver, "legacy");
    let sha1 = "0123456789abcdef0123456789abcdef01234567".to_string();
    let copy = GalleryCopy {
        locator: "content://media/external_primary/images/media/7".into(),
        sha256: digest(b"gallery copy"),
        size: 12,
        display_name: Some("IMG_0001.JPG".into()),
        sha1: Some(sha1.clone()),
    };
    receiver.record_gallery_copy(&id, &copy).unwrap();
    let mut old = copy.clone();
    old.locator = "content://media/external_primary/images/media/8".into();
    old.sha1 = None;
    receiver.record_gallery_copy(&legacy, &old).unwrap();
    let shared = Arc::new(Mutex::new(receiver));
    let router = backupduck_transport::shared_router(shared.clone(), TOKEN).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap()
    });
    let client = Client::new(&base, TOKEN).unwrap();
    let caps = client.capabilities().await.unwrap();
    assert!(caps.cloud_audit && caps.cloud_preexisting);

    let page = client.cloud_due(None, 100, true).await.unwrap();
    assert_eq!(page.next, None);
    assert_eq!(page.items.len(), 1);
    let item = &page.items[0];
    assert_eq!(
        (
            item.asset_id.as_str(),
            item.sha1.as_str(),
            item.sha256.as_str()
        ),
        (id.as_str(), sha1.as_str(), copy.sha256.as_str())
    );
    assert_eq!(
        (item.size, item.kind.as_str(), item.cloud_state.as_str()),
        (12, "photo", "pending")
    );
    // Freshly published copies are not due before ten minutes.
    assert!(client
        .cloud_due(None, 100, false)
        .await
        .unwrap()
        .items
        .is_empty());

    let summary = client
        .post_cloud_observations(&[
            CloudObservation {
                asset_id: id.clone(),
                sha1: sha1.clone(),
                result: CloudResult::Free,
                media_key: Some("AF1Qip-example".into()),
                device_model: Some("Pixel 9".into()),
            },
            CloudObservation {
                asset_id: legacy.clone(),
                sha1: sha1.clone(),
                result: CloudResult::Free,
                media_key: None,
                device_model: None,
            },
        ])
        .await
        .unwrap();
    assert_eq!(
        summary,
        CloudSummary {
            verified: 1,
            rejected: 1,
            ..Default::default()
        }
    );
    assert_eq!(
        client.status(&id).await.unwrap().cloud_state.as_deref(),
        Some("verified")
    );
    assert_eq!(
        shared.lock().unwrap().overview().unwrap()["cloud_verified"],
        1
    );

    let http = reqwest::Client::new();
    let unauthorized = http
        .get(format!("{base}/v2/publications?cloud=all"))
        .send()
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), 401);
    let unauthorized = http
        .post(format!("{base}/v2/cloud-observations"))
        .json(&serde_json::json!({"observations":[]}))
        .send()
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), 401);
    for query in [
        "cloud=some",
        "cloud=all&limit=0",
        "cloud=all&limit=101",
        "cloud=all&after=x",
    ] {
        let response = http
            .get(format!("{base}/v2/publications?{query}"))
            .bearer_auth(TOKEN)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 400, "{query}");
    }
    let observation = serde_json::json!({"asset_id":id,"sha1":sha1,"result":"not_found"});
    let too_many = serde_json::json!({ "observations": vec![observation; 101] });
    let response = http
        .post(format!("{base}/v2/cloud-observations"))
        .bearer_auth(TOKEN)
        .json(&too_many)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 400);
    let oversized = vec![b' '; MAX_CLOUD_OBSERVATION_BYTES + 1];
    let response = http
        .post(format!("{base}/v2/cloud-observations"))
        .bearer_auth(TOKEN)
        .body(oversized)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 400);
    let response = http
        .post(format!("{base}/v2/cloud-observations"))
        .bearer_auth(TOKEN)
        .json(&serde_json::json!({"observations":[{"asset_id":id,"sha1":sha1,"result":"maybe"}]}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 400);
    // A verified copy does not regress on a later miss.
    let summary = client
        .post_cloud_observations(&[CloudObservation {
            asset_id: id.clone(),
            sha1,
            result: CloudResult::NotFound,
            media_key: None,
            device_model: None,
        }])
        .await
        .unwrap();
    assert_eq!(summary.verified, 1);
}
