//! Helpers for capturing exact wire-level HTTP responses from generated smithy-rs servers.

use bytes::Bytes;
use http_body_util::BodyExt;

/// Collect a response into (status, headers-dump, body bytes) and print everything, labeled.
pub async fn dump_response<B>(label: &str, response: http::Response<B>) -> Bytes
where
    B: http_body::Body,
    B::Error: std::fmt::Debug,
{
    let (parts, body) = response.into_parts();
    let body = body.collect().await.expect("failed to collect body").to_bytes();

    println!("\n===== CAPTURE: {label} =====");
    println!("STATUS: {}", parts.status);
    let mut headers: Vec<String> = parts
        .headers
        .iter()
        .map(|(k, v)| format!("  {}: {}", k, String::from_utf8_lossy(v.as_bytes())))
        .collect();
    headers.sort();
    println!("HEADERS ({}):", parts.headers.len());
    for h in &headers {
        println!("{h}");
    }
    println!("BODY ({} bytes)", body.len());
    match std::str::from_utf8(&body) {
        Ok(s) => println!("BODY(utf8): {s}"),
        Err(_) => println!("BODY(utf8): <non-utf8>"),
    }
    println!("BODY(hex): {}", hex(&body));
    println!("===== END: {label} =====");
    body
}

/// Print a CBOR body in diagnostic notation.
pub fn dump_cbor_diag(label: &str, body: &[u8]) {
    println!("CBOR-DIAG [{label}]: {}", minicbor::display(body));
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join("")
}
