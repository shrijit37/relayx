//! Benchmark: gateway-added latency overhead.
//!
//! Measures direct (client → mock) vs gateway (client → gateway → mock)
//! latency to isolate gateway overhead. The mock + gateway are spawned
//! once per benchmark run, and a persistent hyper client is reused across
//! iterations so measurement reflects steady-state pooled latency.
//!
//! Performance gates (from docs/performance.md):
//!   Simple proxy p50 < 1 ms
//!   Simple proxy p95 < 2 ms
//!   Simple proxy p99 < 5 ms

use std::net::SocketAddr;
use std::sync::Arc;

use criterion::{Criterion, criterion_group, criterion_main};
use tokio::runtime::Runtime;

mod harness;

/// Endpoints a benchmark drives requests against.
struct Endpoints {
    direct: SocketAddr,
    via_gateway: SocketAddr,
}

/// Spawn a fresh json mock + a gateway routing to it, once per run.
fn setup(rt: &Runtime) -> anyhow::Result<Endpoints> {
    const BODY: &str =
        r#"{"ok":false,"a":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#;
    rt.block_on(async {
        let mock = harness::spawn_json_mock(BODY).await?;
        let gateway = harness::spawn_gateway(mock.addr).await?;
        Ok::<Endpoints, anyhow::Error>(Endpoints {
            direct: mock.addr,
            via_gateway: gateway,
        })
    })
}

/// Spawn SSE mock + gateway for streaming benchmarks.
fn setup_sse(rt: &Runtime) -> anyhow::Result<Endpoints> {
    rt.block_on(async {
        let mock = mock_upstream::spawn_mock(mock_upstream::MockConfig {
            mode: mock_upstream::MockMode::Sse,
            chunks: 50,
            chunk_size: 128,
            chunk_delay: std::time::Duration::ZERO,
            ..Default::default()
        })
        .await?;
        let gateway = harness::spawn_gateway(mock.addr).await?;
        Ok::<Endpoints, anyhow::Error>(Endpoints {
            direct: mock.addr,
            via_gateway: gateway,
        })
    })
}

fn bench_proxy_overhead(c: &mut Criterion) {
    let rt = match Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("failed to create tokio runtime: {e:#}");
            return;
        }
    };

    let endpoints = match setup(&rt) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("benchmark setup failed: {e:#}");
            return;
        }
    };

    let direct_url = Arc::new(format!("http://{}/v1/echo", endpoints.direct));
    let gateway_url = Arc::new(format!("http://{}/v1/echo", endpoints.via_gateway));

    let make_client = || {
        hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
            .build_http()
    };

    let mut group = c.benchmark_group("simple_proxy");
    group.sample_size(200);

    let rt_handle_direct = rt.handle().clone();
    let rt_handle_gateway = rt.handle().clone();

    // Direct path: client → mock.
    let direct_client = make_client();
    group.bench_function("direct", move |b| {
        b.iter(|| {
            let url = &direct_url;
            std::hint::black_box(rt_handle_direct.block_on(async {
                let req = http::Request::builder()
                    .method(http::Method::POST)
                    .uri(url.as_str())
                    .header(http::header::CONTENT_TYPE, "application/json")
                    .body(axum::body::Body::from(br#"{"n":1}"#.to_vec()))
                    .map_err(|e| e.to_string())?;
                let resp = direct_client
                    .request(req)
                    .await
                    .map_err(|e| e.to_string())?;
                let collected = http_body_util::BodyExt::collect(resp.into_body())
                    .await
                    .map_err(|e| e.to_string())?;
                let _bytes = collected.to_bytes();
                Ok::<(), String>(())
            }))
        })
    });

    // Gateway path: client → gateway → mock.
    let gateway_client = make_client();
    group.bench_function("via_gateway", move |b| {
        b.iter(|| {
            let url = &gateway_url;
            std::hint::black_box(rt_handle_gateway.block_on(async {
                let req = http::Request::builder()
                    .method(http::Method::POST)
                    .uri(url.as_str())
                    .header(http::header::CONTENT_TYPE, "application/json")
                    .body(axum::body::Body::from(br#"{"n":1}"#.to_vec()))
                    .map_err(|e| e.to_string())?;
                let resp = gateway_client
                    .request(req)
                    .await
                    .map_err(|e| e.to_string())?;
                let collected = http_body_util::BodyExt::collect(resp.into_body())
                    .await
                    .map_err(|e| e.to_string())?;
                let _bytes = collected.to_bytes();
                Ok::<(), String>(())
            }))
        })
    });

    group.finish();

    // ── Streaming benchmark ──────────────────────────────────────────────────
    let sse_endpoints = match setup_sse(&rt) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("SSE benchmark setup failed: {e:#}");
            return;
        }
    };

    let sse_direct_url = Arc::new(format!(
        "http://{}/v1/chat/completions",
        sse_endpoints.direct
    ));
    let sse_gateway_url = Arc::new(format!(
        "http://{}/v1/chat/completions",
        sse_endpoints.via_gateway
    ));

    let mut sse_group = c.benchmark_group("streaming_proxy");
    sse_group.sample_size(50);

    let rt_handle_sse_direct = rt.handle().clone();
    let rt_handle_sse_gateway = rt.handle().clone();

    // Direct SSE: client → mock.
    let sse_direct_client = make_client();
    sse_group.bench_function("direct_sse", move |b| {
        b.iter(|| {
            let url = &sse_direct_url;
            std::hint::black_box(rt_handle_sse_direct.block_on(async {
                let req = http::Request::builder()
                    .method(http::Method::POST)
                    .uri(url.as_str())
                    .header(http::header::CONTENT_TYPE, "application/json")
                    .body(axum::body::Body::from(br#"{"stream":true}"#.to_vec()))
                    .map_err(|e| e.to_string())?;
                let resp = sse_direct_client
                    .request(req)
                    .await
                    .map_err(|e| e.to_string())?;
                let collected = http_body_util::BodyExt::collect(resp.into_body())
                    .await
                    .map_err(|e| e.to_string())?;
                let _bytes = collected.to_bytes();
                Ok::<(), String>(())
            }))
        })
    });

    // Gateway SSE: client → gateway → mock.
    let sse_gateway_client = make_client();
    sse_group.bench_function("via_gateway_sse", move |b| {
        b.iter(|| {
            let url = &sse_gateway_url;
            std::hint::black_box(rt_handle_sse_gateway.block_on(async {
                let req = http::Request::builder()
                    .method(http::Method::POST)
                    .uri(url.as_str())
                    .header(http::header::CONTENT_TYPE, "application/json")
                    .body(axum::body::Body::from(br#"{"stream":true}"#.to_vec()))
                    .map_err(|e| e.to_string())?;
                let resp = sse_gateway_client
                    .request(req)
                    .await
                    .map_err(|e| e.to_string())?;
                let collected = http_body_util::BodyExt::collect(resp.into_body())
                    .await
                    .map_err(|e| e.to_string())?;
                let _bytes = collected.to_bytes();
                Ok::<(), String>(())
            }))
        })
    });

    sse_group.finish();
}

criterion_group!(benches, bench_proxy_overhead);
criterion_main!(benches);
