//! Concurrency / load tests.
//!
//! Sends many concurrent requests through the gateway and asserts:
//! - all complete successfully
//! - connection reuse happens (few connections serve many requests)
//! - no errors under moderate load

use std::sync::Arc;

use test_harness::spawn_sse_stack;

/// Send `n` concurrent POSTs through the gateway, returning stats.
async fn concurrent_chat(n: usize, gateway: &std::net::SocketAddr) -> anyhow::Result<()> {
    let url = format!("http://{}/v1/chat/completions", gateway);
    let url = Arc::new(url);

    let mut tasks = Vec::with_capacity(n);
    for _ in 0..n {
        let url = Arc::clone(&url);
        tasks.push(tokio::spawn(async move {
            let client =
                hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
                    .build_http();
            let req = http::Request::builder()
                .method(http::Method::POST)
                .uri(url.as_str())
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(axum::body::Body::from(br#"{"stream":true}"#.to_vec()))?;
            let resp = client.request(req).await?;
            let status = resp.status();
            let collected = http_body_util::BodyExt::collect(resp.into_body()).await?;
            let body = collected.to_bytes();
            anyhow::ensure!(status == http::StatusCode::OK, "unexpected status {status}");
            anyhow::ensure!(!body.is_empty(), "empty response body");
            Ok::<(), anyhow::Error>(())
        }));
    }

    for task in tasks {
        task.await??;
    }
    Ok(())
}

#[tokio::test]
async fn load_one_request() -> anyhow::Result<()> {
    let stack = spawn_sse_stack(3, 32).await?;
    concurrent_chat(1, &stack.gateway.proxy).await
}

#[tokio::test]
async fn load_ten_concurrent() -> anyhow::Result<()> {
    let stack = spawn_sse_stack(3, 32).await?;
    concurrent_chat(10, &stack.gateway.proxy).await
}

#[tokio::test]
async fn load_hundred_concurrent() -> anyhow::Result<()> {
    let stack = spawn_sse_stack(3, 32).await?;
    concurrent_chat(100, &stack.gateway.proxy).await
}

#[tokio::test]
async fn load_connections_are_reused() -> anyhow::Result<()> {
    let stack = spawn_sse_stack(3, 32).await?;

    for _ in 0..50 {
        concurrent_chat(1, &stack.gateway.proxy).await?;
    }

    let served = stack
        .mock
        .state
        .requests_served
        .load(std::sync::atomic::Ordering::Relaxed);
    let accepted = stack
        .mock
        .state
        .connections_accepted
        .load(std::sync::atomic::Ordering::Relaxed);

    assert!(
        served >= 50,
        "mock should have served at least 50 requests, got {served}"
    );
    assert!(
        accepted > 0 && accepted < 50,
        "expected connection reuse (1..50 accepted for 50 requests), got {accepted}"
    );

    Ok(())
}
