mod common;

use common::DaemonHarness;
use std::collections::HashSet;
use tokio::net::TcpListener;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reserve_port_returns_distinct_ports_under_concurrency()
-> Result<(), Box<dyn std::error::Error>> {
    let mut tasks = Vec::new();
    for _ in 0..16 {
        tasks.push(tokio::spawn(async {
            common::reserve_port().await.map_err(|e| e.to_string())
        }));
    }

    let mut ports = HashSet::new();
    for task in tasks {
        let port = task.await??;
        assert!(
            ports.insert(port),
            "reserve_port returned duplicate port {port}"
        );
    }
    assert_eq!(ports.len(), 16);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn daemon_harness_retries_when_first_port_choice_is_occupied()
-> Result<(), Box<dyn std::error::Error>> {
    let occupied = TcpListener::bind("127.0.0.1:0").await?;
    let occupied_port = occupied.local_addr()?.port();

    let mut harness = DaemonHarness::spawn_with_first_port_choice(Some(occupied_port)).await?;
    assert_ne!(
        harness.base_url,
        format!("http://127.0.0.1:{occupied_port}")
    );

    let health = harness.health().await?;
    assert_eq!(health.worker_statuses.api, "idle");

    drop(occupied);
    harness.shutdown().await;
    Ok(())
}
