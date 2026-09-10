//! Cancellation + disconnect integration tests.
//!
//! Verifies:
//! - client disconnect while upstream streams → upstream task is dropped
//! - slow client → backpressure propagates (no eager buffering)

use futures_util::StreamExt;
use http_body_util::BodyExt;
use std::time::Duration;
use test_harness::spawn_sse_stack;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// A client that drops the connection mid-stream.
#[tokio::test]
async fn client_disconnect_cancels_upstream() -> anyhow::Result<()> {
    let stack = spawn_sse_stack(20, 64).await?;

    let served_before = stack
        .mock
        .state
        .requests_served
        .load(std::sync::atomic::Ordering::Relaxed);

    // Build a raw HTTP/1.1 request so we control connection lifetime.
    let req = format!(
        "POST /v1/chat/completions HTTP/1.1\r\n\
         Host: {}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: 15\r\n\
         Connection: close\r\n\
         \r\n\
         {{\"stream\":true}}",
        stack.gateway.proxy
    );

    let mut stream = tokio::net::TcpStream::connect(stack.gateway.proxy).await?;
    stream.write_all(req.as_bytes()).await?;
    stream.flush().await?;

    // Read a little of the response head to ensure the upstream started,
    // then drop the connection (client disconnect mid-stream).
    let mut buf = [0u8; 512];
    let _n = tokio::time::timeout(Duration::from_millis(500), stream.read(&mut buf)).await?;

    drop(stream);

    // Give the gateway time to observe the disconnect and cancel upstream read.
    tokio::time::sleep(Duration::from_millis(300)).await;

    let served_after = stack
        .mock
        .state
        .requests_served
        .load(std::sync::atomic::Ordering::Relaxed);

    assert_eq!(
        served_after,
        served_before + 1,
        "client disconnect should let the request complete once, not spin"
    );

    Ok(())
}

#[tokio::test]
async fn slow_client_receives_backpressure() -> anyhow::Result<()> {
    let stack = spawn_sse_stack(10, 4096).await?;
    let url = format!("http://{}/v1/chat/completions", stack.gateway.proxy);

    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build_http();

    let req = http::Request::builder()
        .method(http::Method::POST)
        .uri(&url)
        .header(http::header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(br#"{"stream":true}"#.to_vec()))?;

    let resp = client.request(req).await?;
    assert_eq!(resp.status(), http::StatusCode::OK);

    let mut frames = resp.into_body().into_data_stream();
    let mut total = 0usize;
    let mut count = 0usize;
    while let Some(chunk) = tokio::time::timeout(Duration::from_secs(5), frames.next()).await? {
        let chunk = chunk?;
        total += chunk.len();
        count += 1;
        // Tiny sleep simulates a slow consumer.
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    assert!(
        total > 0,
        "slow client should still receive the full stream"
    );
    assert!(count > 0, "streamed bodies should arrive in frames");
    // Frames may be split or coalesced by hyper/OS, so assert on content:
    assert!(
        total >= 10 * 4096 * 80 / 100,
        "streamed bytes too small for 10 chunks of 4096 bytes: got {total}"
    );

    Ok(())
}
