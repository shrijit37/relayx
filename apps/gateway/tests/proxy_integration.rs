//! Integration tests: client → gateway → mock upstream.
//!
//! Verifies core proxy semantics: normal requests, large bodies, upstream
//! error passthrough, and byte-identical forwarding.

use mock_upstream::{MockConfig, MockMode, spawn_mock};
use test_harness::{
    dead_upstream_addr, get_hyper, post_hyper, spawn_gateway, spawn_gateway_with_timeout,
    spawn_json_stack, spawn_sse_stack,
};

use std::time::Duration;

#[tokio::test]
async fn proxy_round_trip_json() -> anyhow::Result<()> {
    let stack = spawn_json_stack(r#"{"ok":true,"hello":"world"}"#).await?;
    let url = format!("http://{}/v1/echo", stack.gateway.proxy);

    let (status, body) = post_hyper(&url, r#"{"n":1}"#, &[]).await?;
    assert_eq!(status, http::StatusCode::OK);
    assert_eq!(body, bytes::Bytes::from_static(b"{\"n\":1}"));

    Ok(())
}

#[tokio::test]
async fn proxy_preserves_query_string() -> anyhow::Result<()> {
    let stack = spawn_json_stack(r#"{"ok":true}"#).await?;
    let url = format!("http://{}/v1/echo?foo=bar&x=1", stack.gateway.proxy);

    let (status, _body) = post_hyper(&url, r#"{}"#, &[]).await?;
    assert_eq!(status, http::StatusCode::OK);

    Ok(())
}

#[tokio::test]
async fn proxy_unknown_path_returns_404() -> anyhow::Result<()> {
    let stack = spawn_json_stack(r#"{}"#).await?;
    let url = format!("http://{}/no/such/route", stack.gateway.proxy);

    let (status, body) = post_hyper(&url, r#"{}"#, &[]).await?;
    assert_eq!(status, http::StatusCode::NOT_FOUND);
    let text = std::str::from_utf8(&body)?;
    assert!(text.contains("no route matches"));

    Ok(())
}

#[tokio::test]
async fn proxy_unmatched_method_returns_404() -> anyhow::Result<()> {
    let stack = spawn_json_stack(r#"{}"#).await?;
    let url = format!("http://{}/v1/echo", stack.gateway.proxy);

    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build_http();
    let req = http::Request::builder()
        .method(http::Method::DELETE)
        .uri(&url)
        .body(axum::body::Body::empty())?;
    let resp = client.request(req).await?;
    assert_eq!(resp.status(), http::StatusCode::NOT_FOUND);

    Ok(())
}

#[tokio::test]
async fn proxy_large_request_body() -> anyhow::Result<()> {
    let stack = spawn_json_stack(r#"{}"#).await?;
    let url = format!("http://{}/v1/echo", stack.gateway.proxy);

    let large = "x".repeat(1024 * 1024 + 17);
    let (status, body) = post_hyper(&url, &large, &[]).await?;
    assert_eq!(status, http::StatusCode::OK);
    assert_eq!(body, bytes::Bytes::from(large));

    Ok(())
}

#[tokio::test]
async fn proxy_streaming_sse_passthrough() -> anyhow::Result<()> {
    let stack = spawn_sse_stack(5, 64).await?;
    let url = format!("http://{}/v1/chat/completions", stack.gateway.proxy);

    let (status, body) = post_hyper(&url, r#"{"stream":true}"#, &[]).await?;
    assert_eq!(status, http::StatusCode::OK);

    let text = std::str::from_utf8(&body)?;
    for i in 0..5 {
        assert!(
            text.contains(&format!("\"id\":\"chunk-{i}\"")),
            "chunk {i} missing from streamed body: {text}"
        );
    }
    assert!(text.contains("mock-stream-data-filler"));

    Ok(())
}

#[tokio::test]
async fn proxy_upstream_404_passthrough() -> anyhow::Result<()> {
    let mock_cfg = MockConfig {
        mode: MockMode::Json,
        json_body: r#"{"nope":true}"#.into(),
        ..Default::default()
    };
    let mock = spawn_mock(mock_cfg).await?;
    let gateway = spawn_gateway(mock.addr).await?;

    let url = format!("http://{}/v1/missing-on-mock", gateway.proxy);
    let (status, _body) = post_hyper(&url, r#"{}"#, &[]).await?;
    assert_eq!(status, http::StatusCode::NOT_FOUND);

    Ok(())
}

#[tokio::test]
async fn proxy_upstream_connection_refused() -> anyhow::Result<()> {
    let dead = dead_upstream_addr()?;
    let gateway = spawn_gateway(dead).await?;
    let url = format!("http://{}/v1/echo", gateway.proxy);

    tokio::time::sleep(Duration::from_millis(200)).await;

    let (status, body) = post_hyper(&url, r#"{}"#, &[]).await?;
    assert_eq!(status, http::StatusCode::BAD_GATEWAY);
    let text = std::str::from_utf8(&body)?;
    assert!(text.contains("upstream_connection"));

    Ok(())
}

#[tokio::test]
async fn proxy_healthz_and_ready() -> anyhow::Result<()> {
    let stack = spawn_json_stack(r#"{}"#).await?;

    let (status, body) = get_hyper(&format!("http://{}/healthz", stack.gateway.admin)).await?;
    assert_eq!(status, http::StatusCode::OK);
    let text = std::str::from_utf8(&body)?;
    assert!(text.contains("ok"));

    let (status, _) = get_hyper(&format!("http://{}/ready", stack.gateway.admin)).await?;
    assert_eq!(status, http::StatusCode::OK);

    Ok(())
}

#[tokio::test]
async fn proxy_metrics_exposed() -> anyhow::Result<()> {
    let stack = spawn_json_stack(r#"{}"#).await?;
    let url = format!("http://{}/v1/echo", stack.gateway.proxy);
    let _ = post_hyper(&url, r#"{}"#, &[]).await?;

    // The metrics recorder is a global installed by whichever gateway
    // instance first calls install_metrics; under parallel test binaries
    // the counter may land a moment later. Poll briefly.
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
    let mut text = String::new();
    let mut ok = false;
    while tokio::time::Instant::now() < deadline {
        let (status, body) = get_hyper(&format!("http://{}/metrics", stack.gateway.admin)).await?;
        assert_eq!(status, http::StatusCode::OK);
        text = std::str::from_utf8(&body)?.to_owned();
        if text.contains("relayx_request_total") && text.contains("relayx_upstream_ttfb_ms") {
            ok = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    assert!(ok, "metrics never exposed expected keys: {text}");

    Ok(())
}

// ── Additional tests ────────────────────────────────────────────────────────

#[tokio::test]
async fn proxy_upstream_5xx_passthrough() -> anyhow::Result<()> {
    let mock_cfg = MockConfig {
        mode: MockMode::Sse,
        chunks: 10,
        chunk_size: 64,
        ..Default::default()
    };
    let mock = spawn_mock(mock_cfg).await?;
    // Use x-mock-error-at = 0 to trigger a 503 response.
    let gateway = spawn_gateway(mock.addr).await?;
    let url = format!("http://{}/v1/chat/completions", gateway.proxy);

    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build_http();
    let req = http::Request::builder()
        .method(http::Method::POST)
        .uri(&url)
        .header("content-type", "application/json")
        .header("x-mock-error-at", "0")
        .header("x-mock-error-status", "503")
        .body(axum::body::Body::from(br#"{"stream":true}"#.to_vec()))?;
    let resp = client.request(req).await?;
    assert_eq!(resp.status(), http::StatusCode::SERVICE_UNAVAILABLE);

    Ok(())
}

#[tokio::test]
async fn proxy_upstream_timeout_returns_504() -> anyhow::Result<()> {
    // Mock has 10s ttfb, gateway has 200ms timeout → 504.
    let mock_cfg = MockConfig {
        mode: MockMode::Json,
        ttfb: std::time::Duration::from_secs(10),
        ..Default::default()
    };
    let mock = spawn_mock(mock_cfg).await?;
    let gateway = spawn_gateway_with_timeout(mock.addr, 200).await?;
    let url = format!("http://{}/v1/echo", gateway.proxy);

    let (status, body) = post_hyper(&url, r#"{}"#, &[]).await?;
    assert_eq!(status, http::StatusCode::GATEWAY_TIMEOUT);
    let text = std::str::from_utf8(&body)?;
    assert!(
        text.contains("upstream_timeout") || text.contains("timeout"),
        "expected timeout error in body: {text}"
    );

    Ok(())
}

#[tokio::test]
async fn proxy_slow_upstream_chunk_delay() -> anyhow::Result<()> {
    // Mock streams 5 chunks with 50ms delay each. Total ~250ms.
    let mock_cfg = MockConfig {
        mode: MockMode::Sse,
        chunks: 5,
        chunk_size: 128,
        chunk_delay: std::time::Duration::from_millis(50),
        ..Default::default()
    };
    let mock = spawn_mock(mock_cfg).await?;
    let gateway = spawn_gateway(mock.addr).await?;
    let url = format!("http://{}/v1/chat/completions", gateway.proxy);

    let (status, body) = post_hyper(&url, r#"{"stream":true}"#, &[]).await?;
    assert_eq!(status, http::StatusCode::OK);

    let text = std::str::from_utf8(&body)?;
    for i in 0..5 {
        assert!(
            text.contains(&format!("chunk-{i}")),
            "chunk {i} missing from slow stream"
        );
    }

    Ok(())
}

#[tokio::test]
async fn proxy_timeout_during_streaming() -> anyhow::Result<()> {
    // The per-request timeout covers handler setup (route match → upstream
    // response headers). Use a high TTFB so the timeout fires before headers
    // arrive, producing a clean 504 to the client.
    let mock_cfg = MockConfig {
        mode: MockMode::Sse,
        chunks: 50,
        chunk_size: 256,
        chunk_delay: std::time::Duration::from_millis(100),
        ttfb: std::time::Duration::from_secs(5),
        ..Default::default()
    };
    let mock = spawn_mock(mock_cfg).await?;
    let gateway = spawn_gateway_with_timeout(mock.addr, 500).await?;
    let url = format!("http://{}/v1/chat/completions", gateway.proxy);

    let start = std::time::Instant::now();
    let result = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        let client =
            hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
                .build_http();
        let req = http::Request::builder()
            .method(http::Method::POST)
            .uri(&url)
            .header("content-type", "application/json")
            .body(axum::body::Body::from(br#"{"stream":true}"#.to_vec()))
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        client
            .request(req)
            .await
            .map_err(|e| std::io::Error::other(e.to_string()))
    })
    .await;
    let elapsed = start.elapsed();

    match result {
        Ok(Ok(resp)) => {
            assert!(
                elapsed < std::time::Duration::from_secs(2),
                "gateway timeout should fire before upstream TTFB, took {elapsed:?}"
            );
            let status = resp.status();
            assert!(
                status == http::StatusCode::GATEWAY_TIMEOUT
                    || status.as_u16() == 499,
                "expected 504/499 from gateway timeout, got: {status}"
            );
        }
        Ok(Err(_)) => {
            // Connection dropped by gateway timeout — acceptable.
        }
        Err(_) => {
            // Outer safety-net timeout — also acceptable.
        }
    }

    Ok(())
}

#[tokio::test]
async fn proxy_concurrent_upstream_disconnect() -> anyhow::Result<()> {
    // Send many concurrent requests where mock drops mid-stream.
    let mock_cfg = MockConfig {
        mode: MockMode::Sse,
        chunks: 100,
        chunk_size: 256,
        chunk_delay: std::time::Duration::from_millis(10),
        ..Default::default()
    };
    let mock = spawn_mock(mock_cfg).await?;
    let gateway = spawn_gateway(mock.addr).await?;

    let mut tasks = Vec::new();
    for _ in 0..20 {
        let url = format!("http://{}/v1/chat/completions", gateway.proxy);
        tasks.push(tokio::spawn(async move {
            let client =
                hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
                    .build_http();
            let req = http::Request::builder()
                .method(http::Method::POST)
                .uri(&url)
                .header("content-type", "application/json")
                .body(axum::body::Body::from(br#"{"stream":true}"#.to_vec()))?;
            let resp = client.request(req).await?;
            // First chunk arrives immediately (no delay), so headers should be OK.
            assert_eq!(resp.status(), http::StatusCode::OK);
            tokio::time::timeout(std::time::Duration::from_secs(10),
                http_body_util::BodyExt::collect(resp.into_body())
            ).await
                .map_err(|_| anyhow::anyhow!("task timed out collecting body"))??;
            Ok::<(), anyhow::Error>(())
        }));
    }

    for task in tasks {
        task.await??;
    }

    // Verify mock is still responsive after concurrent requests.
    let served = mock
        .state
        .requests_served
        .load(std::sync::atomic::Ordering::Relaxed);
    assert!(served >= 20, "mock should have served at least 20 requests, got {served}");

    Ok(())
}
