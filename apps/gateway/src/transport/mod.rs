//! Transport-level utilities: header filtering and byte passthrough.
//!
//! These live on the hot path — they must not allocate on the data path
//! beyond what the surrounding hyper/axum machinery already does.

use bytes::Bytes;
use http::header::{CONNECTION, CONTENT_LENGTH, HOST, HeaderName, TE, TRANSFER_ENCODING, UPGRADE};
use http::{HeaderMap, HeaderValue};

/// Hop-by-hop headers that must never be forwarded between client and upstream.
///
/// Per RFC 9110 §7.6.1 these are consumed by a single transport hop and
/// must be stripped when relaying a message.
const HOP_BY_HOP: &[HeaderName] = &[
    CONNECTION,
    // keep-alive is named via Connection on most stacks;
    // proxy-connection is the HTTP/1.0 degenerate spelling.
    HOST, // re-derived from the lane base_url, never forwarded verbatim
    TE,
    TRANSFER_ENCODING,
    UPGRADE,
];

/// Copy headers from `src` into `dst`, skipping hop-by-hop headers.
///
/// `dst` is expected to be empty (or pre-seeded with headers the caller
/// wants to keep). This is allocation-free beyond the header names/values
/// already owned by `src`.
pub fn copy_headers(src: &HeaderMap, dst: &mut HeaderMap) {
    for (name, value) in src.iter() {
        if is_hop_by_hop(name) {
            continue;
        }
        dst.insert(name, value.clone());
    }
}

/// Returns true if `name` is a hop-by-hop header that must be stripped.
///
/// Also covers the proxy-* and *-Connection extension instructions that
/// RFC 9110 reserves for hop-by-hop signaling.
pub fn is_hop_by_hop(name: &HeaderName) -> bool {
    HOP_BY_HOP.contains(name)
        || name.as_str().starts_with("proxy-")
        || name.as_str().starts_with("connection-")
}

/// Strip hop-by-hop headers from a header map (mutates in place).
pub fn strip_hop_by_hop(headers: &mut HeaderMap) {
    let names: Vec<HeaderName> = headers
        .keys()
        .filter(|name| is_hop_by_hop(name))
        .cloned()
        .collect();
    for name in names {
        headers.remove(&name);
    }
}

/// Preserve an upstream `content-length` only when it was exact.
///
/// For proxied responses we generally let hyper recompute framing; this
/// helper exists so tests can assert the common cases.
pub fn has_exact_length(headers: &HeaderMap) -> bool {
    headers
        .get(CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .is_some()
}

/// Value used to overwrite the Host header on forwarded requests.
pub fn build_host_header(scheme: &str, host: &str, port: Option<u16>) -> HeaderValue {
    let mut value = String::with_capacity(host.len() + 8);
    value.push_str(host);
    if let Some(port) = port {
        let default = (scheme == "https" && port == 443) || (scheme == "http" && port == 80);
        if !default {
            value.push(':');
            value.push_str(&port.to_string());
        }
    }
    HeaderValue::from_str(&value).unwrap_or_else(|_| HeaderValue::from_static("localhost"))
}

/// Small helper that lets the streaming path tag byte totals without
/// forcing an allocation per frame.
#[inline]
pub fn frame_len(frames: &[Bytes]) -> usize {
    frames.iter().map(|f| f.len()).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::{HeaderMap, HeaderName, HeaderValue};

    fn header(value: &'static str) -> HeaderValue {
        HeaderValue::from_static(value)
    }

    #[test]
    fn test_strip_hop_by_hop() {
        let mut headers = HeaderMap::new();
        headers.insert(CONNECTION, header("keep-alive"));
        headers.insert(HOST, header("example.com"));
        headers.insert(TRANSFER_ENCODING, header("chunked"));
        headers.insert("x-custom", header("preserve-me"));
        headers.insert("proxy-authorization", header("secret"));

        // mutate in place
        let names: Vec<HeaderName> = headers
            .keys()
            .filter(|name| is_hop_by_hop(name))
            .cloned()
            .collect();
        for name in names {
            headers.remove(&name);
        }

        assert!(headers.get(CONNECTION).is_none());
        assert!(headers.get(HOST).is_none());
        assert!(headers.get(TRANSFER_ENCODING).is_none());
        assert!(headers.get("proxy-authorization").is_none());
        assert_eq!(headers.get("x-custom"), Some(&header("preserve-me")));
    }

    #[test]
    fn test_copy_headers_skips_hop_by_hop() {
        let mut src = HeaderMap::new();
        src.insert(CONNECTION, header("close"));
        src.insert("x-request-id", header("abc123"));
        src.insert("content-type", header("application/json"));

        let mut dst = HeaderMap::new();
        copy_headers(&src, &mut dst);

        assert!(dst.get(CONNECTION).is_none());
        assert_eq!(dst.get("x-request-id"), Some(&header("abc123")));
        assert_eq!(dst.get("content-type"), Some(&header("application/json")));
    }

    #[test]
    fn test_build_host_header() {
        assert_eq!(
            build_host_header("http", "example.com", None),
            "example.com"
        );
        assert_eq!(
            build_host_header("https", "example.com", Some(443)),
            "example.com"
        );
        assert_eq!(
            build_host_header("http", "example.com", Some(8080)),
            "example.com:8080"
        );
    }
}
