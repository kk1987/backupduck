//! Shared helpers for the native integration tests.
#![allow(dead_code)]

use backupduck_native::ReceiverHost;
use std::path::Path;

/// A loopback port that was free a moment ago. Another parallel test can take
/// it between this probe and the real bind, so callers that need a listener
/// retry through `start_receiver` instead of trusting it.
pub fn free_address() -> std::net::SocketAddr {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
}

/// Windows reports a lost race as WSAEADDRINUSE (10048); Unix says "in use".
pub fn address_in_use(error: &str) -> bool {
    error.contains("10048") || error.contains("in use") || error.contains("os error 48")
}

/// Start a receiver on a fresh loopback port, retrying when a parallel test
/// grabbed the probed port first.
pub async fn start_receiver(root: &Path, capacity: u64) -> ReceiverHost {
    let mut last = None;
    for _ in 0..10 {
        match ReceiverHost::start(root, free_address(), capacity).await {
            Ok(host) => return host,
            Err(error) if address_in_use(&format!("{error:?}")) => last = Some(error),
            Err(error) => panic!("receiver start: {error:?}"),
        }
    }
    panic!("no free loopback port after 10 attempts: {last:?}");
}
