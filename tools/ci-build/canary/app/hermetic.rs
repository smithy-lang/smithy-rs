/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

// Hermetic assertions. These run under `cargo test` and may use test-util; nothing
// here is reachable from `cargo build`, which is what keeps COMPILE_FAIL attributable
// to crates a consumer actually compiles (design.md 3.1, 3.3).
//
// No AWS and nothing off-host: the only sockets are loopback listeners these tests
// start themselves.

use aws_credential_types::credential_fn::provide_credentials_fn;
use aws_credential_types::provider::ProvideCredentials;
use aws_sdk_s3::config::{BehaviorVersion, Credentials, IdentityCache, Region};
use aws_smithy_http_client::test_util::{ReplayEvent, StaticReplayClient};
use aws_smithy_types::body::SdkBody;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime};

fn replay_event() -> ReplayEvent {
    ReplayEvent::new(
        http::Request::builder()
            .uri("https://s3.us-east-1.amazonaws.com/")
            .body(SdkBody::empty())
            .unwrap(),
        http::Response::builder()
            .status(200)
            .body(SdkBody::from(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
                 <ListAllMyBucketsResult><Buckets>\
                 <Bucket><Name>canary-bucket</Name><CreationDate>2026-01-01T00:00:00.000Z</CreationDate></Bucket>\
                 </Buckets><Owner><ID>owner-id</ID></Owner></ListAllMyBucketsResult>",
            ))
            .unwrap(),
    )
}

/// Drive a real operation through the whole orchestrator -- endpoint resolution,
/// SigV4 signing, serialization, transmit, deserialization -- with the network
/// replaced by a canned response and credentials hard-coded.
#[tokio::test]
async fn s3_request_response_round_trip() {
    let http_client = StaticReplayClient::new(vec![replay_event()]);
    let config = aws_sdk_s3::Config::builder()
        .behavior_version(BehaviorVersion::latest())
        .credentials_provider(Credentials::new(
            "AKIDCANARY",
            "secret",
            None,
            None,
            "canary",
        ))
        .region(Region::new("us-east-1"))
        .http_client(http_client.clone())
        .build();

    let out = aws_sdk_s3::Client::from_conf(config)
        .list_buckets()
        .send()
        .await
        .expect("ListBuckets should succeed against the canned response");

    let names: Vec<_> = out.buckets().iter().filter_map(|b| b.name()).collect();
    assert_eq!(names, vec!["canary-bucket"], "response deserialized wrong");

    let sent = http_client
        .actual_requests()
        .next()
        .expect("no request sent");
    assert_eq!(
        sent.uri(),
        "https://s3.us-east-1.amazonaws.com/?x-id=ListBuckets",
        "endpoint resolved wrong"
    );
    let auth = sent
        .headers()
        .get("authorization")
        .expect("request was not signed");
    assert!(
        auth.starts_with("AWS4-HMAC-SHA256 Credential=AKIDCANARY/"),
        "unexpected signature scheme: {auth}"
    );
}

/// Gap 1's refresh half. `lazy()` refreshes when an identity is within
/// DEFAULT_BUFFER_TIME (10s, jittered down to 5s) of expiring, so a 2s expiry must
/// always refetch and a 1h expiry must never do so. Both halves are needed: either
/// alone is satisfiable by a broken cache.
#[tokio::test]
async fn credential_refresh_honours_expiry() {
    let short = provider_calls_over_two_requests(Duration::from_secs(2)).await;
    assert!(
        short >= 2,
        "credentials expiring inside the refresh buffer were not refetched: {short} call(s)"
    );

    let long = provider_calls_over_two_requests(Duration::from_secs(3600)).await;
    assert_eq!(
        long, 1,
        "credentials outside the buffer should be cached, but the provider ran {long} time(s)"
    );
}

async fn provider_calls_over_two_requests(ttl: Duration) -> usize {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();

    let provider = provide_credentials_fn(move || {
        let counter = counter.clone();
        async move {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(Credentials::new(
                "AKIDCANARY",
                "secret",
                None,
                Some(SystemTime::now() + ttl),
                "canary-refresh",
            ))
        }
    });

    let http_client = StaticReplayClient::new(vec![replay_event(), replay_event()]);
    let config = aws_sdk_s3::Config::builder()
        .behavior_version(BehaviorVersion::latest())
        .credentials_provider(provider)
        .identity_cache(IdentityCache::lazy().build())
        .region(Region::new("us-east-1"))
        .http_client(http_client)
        .build();
    let client = aws_sdk_s3::Client::from_conf(config);

    for _ in 0..2 {
        client.list_buckets().send().await.expect("canned 200");
    }
    calls.load(Ordering::SeqCst)
}

/// Gap 2's HTTP half, directly rather than as a side effect: point a real S3 client at
/// a loopback listener with **no `http_client` override**, so the default hyper client
/// does the work -- real connect, real HTTP/1.1 framing, real connection pool.
///
/// Pooling is proved by counting TCP *accepts* against requests served. Keep-alive is
/// HTTP/1.1's default, so omitting `Connection: close` is all the server has to do:
/// two requests over one accepted connection means the pool reused it.
#[tokio::test]
async fn real_http_client_connects_and_reuses_one_connection() {
    let body = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
                <ListAllMyBucketsResult><Buckets></Buckets>\
                <Owner><ID>owner-id</ID></Owner></ListAllMyBucketsResult>";
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().unwrap().port();

    let accepts = Arc::new(AtomicUsize::new(0));
    let served = Arc::new(AtomicUsize::new(0));
    let (a, sv) = (accepts.clone(), served.clone());

    let server = thread::spawn(move || {
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/xml\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        while sv.load(Ordering::SeqCst) < 2 {
            let Ok((mut sock, _)) = listener.accept() else {
                return;
            };
            a.fetch_add(1, Ordering::SeqCst);
            // Serve every request that arrives on THIS connection, so a pooled client
            // is not forced to reconnect.
            loop {
                let mut buf = [0u8; 8192];
                match sock.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                if sock.write_all(resp.as_bytes()).is_err() {
                    break;
                }
                let _ = sock.flush();
                if sv.fetch_add(1, Ordering::SeqCst) + 1 >= 2 {
                    break;
                }
            }
        }
    });

    // No .http_client(...) -- this is the real default client.
    let config = aws_sdk_s3::Config::builder()
        .behavior_version(BehaviorVersion::latest())
        .credentials_provider(Credentials::new(
            "AKIDCANARY",
            "secret",
            None,
            None,
            "canary",
        ))
        .region(Region::new("us-east-1"))
        .endpoint_url(format!("http://127.0.0.1:{port}"))
        .build();
    let client = aws_sdk_s3::Client::from_conf(config);

    for i in 0..2 {
        client
            .list_buckets()
            .send()
            .await
            .unwrap_or_else(|e| panic!("request {i} failed against loopback: {e:?}"));
    }
    let _ = server.join();

    assert_eq!(
        served.load(Ordering::SeqCst),
        2,
        "server did not serve two requests"
    );
    assert_eq!(
        accepts.load(Ordering::SeqCst),
        1,
        "connection was NOT reused: {} accepts for 2 requests",
        accepts.load(Ordering::SeqCst)
    );
}

/// Clear every credential-relevant variable so each chain check starts from a known
/// state. These checks mutate process env, so leakage between them would make the
/// result depend on order -- the flakiness the real canary must not ship.
fn reset_credential_env() {
    for k in [
        "AWS_ACCESS_KEY_ID",
        "AWS_SECRET_ACCESS_KEY",
        "AWS_SESSION_TOKEN",
        "AWS_PROFILE",
        "AWS_CONTAINER_CREDENTIALS_FULL_URI",
        "AWS_CONTAINER_CREDENTIALS_RELATIVE_URI",
        "AWS_EC2_METADATA_SERVICE_ENDPOINT",
        "AWS_EC2_METADATA_DISABLED",
    ] {
        std::env::remove_var(k);
    }
    // An empty real file, not `/dev/null`: the null device is `NUL` on Windows, so a
    // hardcoded unix path would mean something different on the Windows and macOS legs.
    let empty = std::env::temp_dir().join("canary-poc-empty-aws-config");
    let _ = std::fs::write(&empty, "");
    std::env::set_var("AWS_CONFIG_FILE", &empty);
    std::env::set_var("AWS_SHARED_CREDENTIALS_FILE", &empty);
}

/// Serve `n` requests on a loopback-only listener, choosing each response body from
/// the request head. 127.0.0.1 with port 0: loopback, kernel-assigned, nothing
/// reachable off-host.
fn serve_loopback<F>(n: usize, respond: F) -> (u16, thread::JoinHandle<()>)
where
    F: Fn(&str) -> String + Send + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().unwrap().port();
    let handle = thread::spawn(move || {
        for _ in 0..n {
            let Ok((mut sock, _)) = listener.accept() else {
                return;
            };
            let mut buf = [0u8; 4096];
            let read = sock.read(&mut buf).unwrap_or(0);
            let head = String::from_utf8_lossy(&buf[..read]).to_string();
            let body = respond(&head);
            // IMDSv2's token response must echo the TTL back or the client rejects it
            // with `TokenErrorKind::NoTtl`. Harmless on the other responses.
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nx-aws-ec2-metadata-token-ttl-seconds: 21600\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = sock.write_all(resp.as_bytes());
            let _ = sock.flush();
        }
    });
    (port, handle)
}

async fn resolve_from_chain() -> aws_credential_types::Credentials {
    let cfg = aws_config::defaults(BehaviorVersion::latest())
        .region(Region::new("us-east-1"))
        .load()
        .await;
    cfg.credentials_provider()
        .expect("a provider chain was built")
        .provide_credentials()
        .await
        .expect("the chain should have resolved")
}

/// Links 2, 3 and 4 of the real chain, in one test because they share process env and
/// must not run concurrently -- cargo runs tests in parallel threads by default.
#[tokio::test]
async fn real_chain_reaches_links_two_three_and_four() {
    // Link 2 -- the shared credentials file. The laptop and EC2-with-profile consumer.
    let path = std::env::temp_dir().join("canary-poc-credentials");
    std::fs::write(
        &path,
        "[default]\naws_access_key_id = ASIAFROMPROFILE\naws_secret_access_key = profile-secret\n",
    )
    .expect("write temp credentials file");
    reset_credential_env();
    std::env::set_var("AWS_SHARED_CREDENTIALS_FILE", &path);
    std::env::set_var("AWS_EC2_METADATA_DISABLED", "true");
    let creds = resolve_from_chain().await;
    assert_eq!(
        creds.access_key_id(),
        "ASIAFROMPROFILE",
        "credentials did not come from the profile file"
    );
    let _ = std::fs::remove_file(&path);

    // Link 3 -- EcsContainer. `validate_full_uri` permits loopback explicitly.
    let (port, server) = serve_loopback(1, |_| {
        r#"{"AccessKeyId":"ASIAFROMECS","SecretAccessKey":"ecs-secret","Token":"ecs-token","Expiration":"2099-01-01T00:00:00Z"}"#.to_string()
    });
    reset_credential_env();
    std::env::set_var("AWS_EC2_METADATA_DISABLED", "true");
    std::env::set_var(
        "AWS_CONTAINER_CREDENTIALS_FULL_URI",
        format!("http://127.0.0.1:{port}/creds"),
    );
    let creds = resolve_from_chain().await;
    assert_eq!(
        creds.access_key_id(),
        "ASIAFROMECS",
        "credentials did not come from the loopback ECS endpoint"
    );
    assert!(
        creds.expiry().is_some(),
        "ECS credentials should carry an expiry"
    );
    let _ = server.join();

    // Link 4 -- IMDS, including the v2 token handshake. Three requests in order:
    //   PUT /latest/api/token
    //   GET /latest/meta-data/iam/security-credentials/          -> profile name
    //   GET /latest/meta-data/iam/security-credentials/{profile} -> credentials JSON
    // Note this must UNSET AWS_EC2_METADATA_DISABLED, which the checks above set only
    // to stop IMDS hanging on a non-EC2 host. Pointing the endpoint at loopback means
    // the path is tested rather than suppressed.
    let (port, server) = serve_loopback(3, |head| {
        if head.starts_with("PUT /latest/api/token") {
            "imds-token-aaaa".to_string()
        } else if head.contains("/latest/meta-data/iam/security-credentials/canary-role") {
            r#"{"Code":"Success","LastUpdated":"2026-01-01T00:00:00Z","Type":"AWS-HMAC","AccessKeyId":"ASIAFROMIMDS","SecretAccessKey":"imds-secret","Token":"imds-token","Expiration":"2099-01-01T00:00:00Z"}"#.to_string()
        } else {
            "canary-role".to_string()
        }
    });
    reset_credential_env();
    std::env::set_var(
        "AWS_EC2_METADATA_SERVICE_ENDPOINT",
        format!("http://127.0.0.1:{port}"),
    );
    let creds = resolve_from_chain().await;
    assert_eq!(
        creds.access_key_id(),
        "ASIAFROMIMDS",
        "credentials did not come from the loopback IMDS endpoint"
    );
    assert!(
        creds.expiry().is_some(),
        "IMDS credentials should carry an expiry"
    );
    let _ = server.join();
}

// ---------------------------------------------------------------------------
// Gap 2, "induced latency and loss". The design calls this row UNCLOSABLE on the
// grounds that "neither phase can make S3 slow or lossy on demand" -- which is true
// of S3 and irrelevant, because the loopback listener above already drives the REAL
// default hyper client (no `http_client` override). A listener we own can be made
// arbitrarily slow and can drop connections at will. These two checks test that.
// ---------------------------------------------------------------------------

/// A listener that sleeps `delay` before responding, then serves `200`.
/// Returns the bound port.
fn serve_slow(delay: Duration) -> (u16, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().unwrap().port();
    let handle = thread::spawn(move || {
        let body = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
                    <ListAllMyBucketsResult><Buckets></Buckets>\
                    <Owner><ID>owner-id</ID></Owner></ListAllMyBucketsResult>";
        if let Ok((mut sock, _)) = listener.accept() {
            let mut buf = [0u8; 8192];
            let _ = sock.read(&mut buf);
            thread::sleep(delay);
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = sock.write_all(resp.as_bytes());
            let _ = sock.flush();
        }
    });
    (port, handle)
}

/// Induced latency. The server takes 1s; the operation timeout is 150ms. A working
/// timeout path must fail FAST -- so the assertion is both the error kind and the
/// elapsed time. Elapsed is what discriminates: if timeouts were broken the request
/// would succeed after 1s instead, which an error-kind-only assertion would miss.
///
/// The server handle is deliberately NOT joined: joining would block the test for the
/// full 1s the client correctly refused to wait for, making a check of fast failure
/// slow. The thread is reaped at process exit.
#[tokio::test]
async fn induced_latency_trips_the_operation_timeout() {
    let (port, _server) = serve_slow(Duration::from_secs(1));

    // No .http_client(...) -- the real hyper client, as in the pooling check.
    let config = aws_sdk_s3::Config::builder()
        .behavior_version(BehaviorVersion::latest())
        .credentials_provider(Credentials::new(
            "AKIDCANARY",
            "secret",
            None,
            None,
            "canary",
        ))
        .region(Region::new("us-east-1"))
        .endpoint_url(format!("http://127.0.0.1:{port}"))
        .timeout_config(
            aws_sdk_s3::config::timeout::TimeoutConfig::builder()
                .operation_timeout(Duration::from_millis(150))
                .build(),
        )
        .retry_config(aws_sdk_s3::config::retry::RetryConfig::disabled())
        .build();

    let started = std::time::Instant::now();
    let result = aws_sdk_s3::Client::from_conf(config)
        .list_buckets()
        .send()
        .await;
    let elapsed = started.elapsed();

    let err = result
        .err()
        .expect("a 1s response under a 150ms timeout must fail");
    assert!(
        matches!(err, aws_sdk_s3::error::SdkError::TimeoutError(_)),
        "expected a timeout, got: {err:?}"
    );
    assert!(
        elapsed < Duration::from_millis(600),
        "timeout did not fire promptly: {elapsed:?} elapsed, so the timeout path is not working"
    );
}

/// Induced loss. The server accepts and drops the first two connections without
/// writing a byte, then serves the third. A working retry path must reconnect and
/// recover, so the assertions are that the call SUCCEEDED and that it took exactly
/// three accepts -- the accept count is what proves retries happened rather than
/// a single lucky attempt.
#[tokio::test]
async fn induced_connection_loss_is_retried_and_recovers() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().unwrap().port();
    let accepts = Arc::new(AtomicUsize::new(0));
    let a = accepts.clone();

    let server = thread::spawn(move || {
        let body = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
                    <ListAllMyBucketsResult><Buckets></Buckets>\
                    <Owner><ID>owner-id</ID></Owner></ListAllMyBucketsResult>";
        for _ in 0..3 {
            let Ok((mut sock, _)) = listener.accept() else {
                return;
            };
            let n = a.fetch_add(1, Ordering::SeqCst) + 1;
            let mut buf = [0u8; 8192];
            let _ = sock.read(&mut buf);
            if n < 3 {
                // Drop mid-exchange: read the request, answer nothing, close.
                // hyper sees EOF before a response -- a transport error, retryable.
                drop(sock);
                continue;
            }
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = sock.write_all(resp.as_bytes());
            let _ = sock.flush();
            return;
        }
    });

    let config = aws_sdk_s3::Config::builder()
        .behavior_version(BehaviorVersion::latest())
        .credentials_provider(Credentials::new(
            "AKIDCANARY",
            "secret",
            None,
            None,
            "canary",
        ))
        .region(Region::new("us-east-1"))
        .endpoint_url(format!("http://127.0.0.1:{port}"))
        .retry_config(
            aws_sdk_s3::config::retry::RetryConfig::standard()
                .with_max_attempts(3)
                .with_initial_backoff(Duration::from_millis(10)),
        )
        .build();

    let out = aws_sdk_s3::Client::from_conf(config)
        .list_buckets()
        .send()
        .await
        .expect("two dropped connections should be retried and the third should succeed");
    assert!(out.buckets().is_empty(), "unexpected body: {out:?}");

    let _ = server.join();
    assert_eq!(
        accepts.load(Ordering::SeqCst),
        3,
        "expected 3 accepts (2 dropped + 1 served); retry did not reconnect"
    );
}

// ---------------------------------------------------------------------------
// Gap 4 — API surface. Four checks: presigning and pagination (measured in an
// earlier prototype but lost when `test-util` moved to [dev-dependencies], so
// re-added here), plus status-code retry classification and multipart, which
// were never built.
// ---------------------------------------------------------------------------

/// 2025-06-15T00:00:00Z. Pinned so the signature scope and X-Amz-Date are fixed.
const PINNED_EPOCH: u64 = 1_749_945_600;

fn canary_s3_config() -> aws_sdk_s3::Config {
    aws_sdk_s3::Config::builder()
        .behavior_version(BehaviorVersion::latest())
        .credentials_provider(Credentials::new(
            "AKIDCANARY",
            "secret",
            None,
            None,
            "canary",
        ))
        .region(Region::new("us-east-1"))
        .build()
}

/// Gap 4's presign, and the structural half of gap 3.
///
/// A presigned URL is pure computation -- no replay client, no network at all.
/// Unlike a header-signed request it is **byte-stable**, because `SignedHeaders`
/// is `host` alone: `x-amz-user-agent` is not signed, so the signature does not
/// move when the SDK version or rustc version changes. That is what makes the
/// signature itself assertable here but not in `s3_request_response_round_trip`.
#[tokio::test]
async fn presigned_url_is_structurally_fixed_and_byte_stable() {
    let presigning = aws_sdk_s3::presigning::PresigningConfig::builder()
        .start_time(std::time::UNIX_EPOCH + Duration::from_secs(PINNED_EPOCH))
        .expires_in(Duration::from_secs(3600))
        .build()
        .expect("presigning config");

    let presign = || async {
        aws_sdk_s3::Client::from_conf(canary_s3_config())
            .get_object()
            .bucket("canary-bucket")
            .key("canary-key")
            .presigned(presigning.clone())
            .await
            .expect("presigning should not fail")
    };

    let uri = presign().await.uri().to_string();

    // Five structural assertions. Each names a distinct thing that could drift.
    for expected in [
        "X-Amz-Algorithm=AWS4-HMAC-SHA256",
        "X-Amz-Credential=AKIDCANARY%2F20250615%2Fus-east-1%2Fs3%2Faws4_request",
        "X-Amz-Date=20250615T000000Z",
        "X-Amz-Expires=3600",
        "X-Amz-SignedHeaders=host",
    ] {
        assert!(
            uri.contains(expected),
            "presigned URL missing {expected}\n  got: {uri}"
        );
    }

    // Determinism: the same inputs must produce the same signature. This is the
    // half that cannot rot -- it compares two live computations rather than a
    // committed literal.
    let again = presign().await.uri().to_string();
    assert_eq!(
        uri, again,
        "presigning is not deterministic under a pinned clock"
    );

    let sig = uri
        .split("X-Amz-Signature=")
        .nth(1)
        .and_then(|s| s.split('&').next())
        .expect("no X-Amz-Signature in presigned URL");
    assert_eq!(
        sig.len(),
        64,
        "signature is not a 64-char hex digest: {sig}"
    );
    assert!(
        sig.chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
        "signature is not lowercase hex: {sig}"
    );

    // Golden value. Safe to commit because every input is version-independent:
    // the full URI is
    //   host canary-bucket.s3.us-east-1.amazonaws.com, path /canary-key,
    //   x-id=GetObject, and the five SigV4 query params
    // with no user-agent, SDK version or rustc version anywhere in the signed set
    // (SignedHeaders is `host` alone, asserted above). So unlike a header-signed
    // request, this does not drift on release.
    //
    // If this ever fails, it is a REAL change to the URL a customer receives --
    // canonicalization, the query set, or the signing math. Investigate before
    // updating the literal; do not update it reflexively.
    assert_eq!(
        sig, "7421d3cde7b28c64fd9908eb5224f09f271bd3e51d64d76a9885e15984f67dc9",
        "presigned signature changed -- the URL a customer gets has changed"
    );
}

fn list_objects_page(keys: &[&str], next_token: Option<&str>) -> http::Response<SdkBody> {
    let contents: String = keys
        .iter()
        .map(|k| format!("<Contents><Key>{k}</Key><Size>1</Size></Contents>"))
        .collect();
    let (truncated, token) = match next_token {
        Some(t) => (
            "true",
            format!("<NextContinuationToken>{t}</NextContinuationToken>"),
        ),
        None => ("false", String::new()),
    };
    http::Response::builder()
        .status(200)
        .body(SdkBody::from(format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
             <ListBucketResult><Name>canary-bucket</Name>\
             <IsTruncated>{truncated}</IsTruncated>{token}{contents}</ListBucketResult>"
        )))
        .unwrap()
}

/// Gap 4's pagination. The paginator is SDK-driven orchestration: it must read
/// `NextContinuationToken` off page 1 and put it on the request for page 2.
///
/// Three assertions, and the third is the one that discriminates: asserting only
/// the keys would pass even if the client had sent the same request twice.
#[tokio::test]
async fn paginator_follows_the_continuation_token_across_two_pages() {
    let http_client = StaticReplayClient::new(vec![
        ReplayEvent::new(
            http::Request::builder()
                .uri("https://canary-bucket.s3.us-east-1.amazonaws.com/?list-type=2")
                .body(SdkBody::empty())
                .unwrap(),
            list_objects_page(&["key-1", "key-2"], Some("TOKEN-PAGE-2")),
        ),
        ReplayEvent::new(
            http::Request::builder()
                .uri("https://canary-bucket.s3.us-east-1.amazonaws.com/?list-type=2")
                .body(SdkBody::empty())
                .unwrap(),
            list_objects_page(&["key-3"], None),
        ),
    ]);

    let config = aws_sdk_s3::Config::builder()
        .behavior_version(BehaviorVersion::latest())
        .credentials_provider(Credentials::new(
            "AKIDCANARY",
            "secret",
            None,
            None,
            "canary",
        ))
        .region(Region::new("us-east-1"))
        .http_client(http_client.clone())
        .build();

    let mut pages = aws_sdk_s3::Client::from_conf(config)
        .list_objects_v2()
        .bucket("canary-bucket")
        .into_paginator()
        .send();

    let mut keys = Vec::new();
    while let Some(page) = pages.next().await {
        let page = page.expect("each page should deserialize");
        keys.extend(
            page.contents()
                .iter()
                .filter_map(|o| o.key().map(str::to_string)),
        );
    }

    assert_eq!(
        keys,
        ["key-1", "key-2", "key-3"],
        "paginator returned wrong keys"
    );

    let sent: Vec<_> = http_client.actual_requests().collect();
    assert_eq!(
        sent.len(),
        2,
        "expected exactly 2 requests, got {}",
        sent.len()
    );
    let second = sent[1].uri();
    assert!(
        second.contains("continuation-token=TOKEN-PAGE-2"),
        "page 2 did not carry the continuation token from page 1: {second}"
    );
}

fn s3_error(status: u16, code: &str) -> http::Response<SdkBody> {
    http::Response::builder()
        .status(status)
        .body(SdkBody::from(format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
             <Error><Code>{code}</Code><Message>canary induced</Message></Error>"
        )))
        .unwrap()
}

/// Gap 4's retry **classification** -- distinct from the transport-retry loop
/// proven by `induced_connection_loss_is_retried_and_recovers`. That check drops
/// TCP connections; this one returns well-formed HTTP errors and asks whether the
/// SDK classifies them correctly.
///
/// Both directions are asserted in one test, because either alone is satisfiable
/// by a broken policy: "retry everything" passes the 503 case, "retry nothing"
/// passes the 400 case. Only the pair pins the behaviour.
#[tokio::test]
async fn status_code_retry_classification_retries_503_but_not_400() {
    let retry = || {
        aws_sdk_s3::config::retry::RetryConfig::standard()
            .with_max_attempts(3)
            .with_initial_backoff(Duration::from_millis(10))
    };

    // Retryable: 503 twice, then success. Three attempts, one success.
    let transient = StaticReplayClient::new(vec![
        ReplayEvent::new(
            http::Request::builder()
                .uri("https://s3.us-east-1.amazonaws.com/")
                .body(SdkBody::empty())
                .unwrap(),
            s3_error(503, "ServiceUnavailable"),
        ),
        ReplayEvent::new(
            http::Request::builder()
                .uri("https://s3.us-east-1.amazonaws.com/")
                .body(SdkBody::empty())
                .unwrap(),
            s3_error(503, "ServiceUnavailable"),
        ),
        replay_event(),
    ]);
    let config = aws_sdk_s3::Config::builder()
        .behavior_version(BehaviorVersion::latest())
        .credentials_provider(Credentials::new(
            "AKIDCANARY",
            "secret",
            None,
            None,
            "canary",
        ))
        .region(Region::new("us-east-1"))
        .http_client(transient.clone())
        .retry_config(retry())
        .build();
    let out = aws_sdk_s3::Client::from_conf(config)
        .list_buckets()
        .send()
        .await
        .expect("503 is retryable, so the third attempt should succeed");
    assert_eq!(
        out.buckets()
            .iter()
            .filter_map(|b| b.name())
            .collect::<Vec<_>>(),
        vec!["canary-bucket"]
    );
    assert_eq!(
        transient.actual_requests().count(),
        3,
        "expected 3 attempts for 503, 503, 200"
    );

    // Not retryable: a 400 must fail on the first attempt. If the policy wrongly
    // retried it, the request count would exceed 1.
    let permanent = StaticReplayClient::new(vec![ReplayEvent::new(
        http::Request::builder()
            .uri("https://s3.us-east-1.amazonaws.com/")
            .body(SdkBody::empty())
            .unwrap(),
        s3_error(400, "InvalidRequest"),
    )]);
    let config = aws_sdk_s3::Config::builder()
        .behavior_version(BehaviorVersion::latest())
        .credentials_provider(Credentials::new(
            "AKIDCANARY",
            "secret",
            None,
            None,
            "canary",
        ))
        .region(Region::new("us-east-1"))
        .http_client(permanent.clone())
        .retry_config(retry())
        .build();
    let err = aws_sdk_s3::Client::from_conf(config)
        .list_buckets()
        .send()
        .await
        .expect_err("a 400 must not succeed");
    assert!(
        err.as_service_error().is_some(),
        "expected a service error for the 400, got {err:?}"
    );
    assert_eq!(
        permanent.actual_requests().count(),
        1,
        "a 400 was retried; it must be classified as non-retryable"
    );
}

/// Gap 4's multipart upload. Four canned responses in sequence, which is what
/// makes the plumbing observable: the `UploadId` from create must appear on both
/// part requests and on complete, and the `ETag`s returned by the parts must be
/// the ones sent to complete. Asserting only "it returned Ok" would pass even if
/// the upload id were dropped.
#[tokio::test]
async fn multipart_upload_plumbs_upload_id_and_etags() {
    let req = || {
        http::Request::builder()
            .uri("https://canary-bucket.s3.us-east-1.amazonaws.com/canary-key")
            .body(SdkBody::empty())
            .unwrap()
    };
    let xml = |body: String| {
        http::Response::builder()
            .status(200)
            .body(SdkBody::from(body))
            .unwrap()
    };

    let http_client = StaticReplayClient::new(vec![
        ReplayEvent::new(
            req(),
            xml(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?><InitiateMultipartUploadResult>\
                 <Bucket>canary-bucket</Bucket><Key>canary-key</Key>\
                 <UploadId>UPLOAD-ID-1</UploadId></InitiateMultipartUploadResult>"
                    .to_string(),
            ),
        ),
        ReplayEvent::new(
            req(),
            http::Response::builder()
                .status(200)
                .header("ETag", "\"etag-part-1\"")
                .body(SdkBody::empty())
                .unwrap(),
        ),
        ReplayEvent::new(
            req(),
            http::Response::builder()
                .status(200)
                .header("ETag", "\"etag-part-2\"")
                .body(SdkBody::empty())
                .unwrap(),
        ),
        ReplayEvent::new(
            req(),
            xml(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?><CompleteMultipartUploadResult>\
                 <Location>https://canary-bucket.s3.amazonaws.com/canary-key</Location>\
                 <Bucket>canary-bucket</Bucket><Key>canary-key</Key>\
                 <ETag>\"etag-final\"</ETag></CompleteMultipartUploadResult>"
                    .to_string(),
            ),
        ),
    ]);

    let config = aws_sdk_s3::Config::builder()
        .behavior_version(BehaviorVersion::latest())
        .credentials_provider(Credentials::new(
            "AKIDCANARY",
            "secret",
            None,
            None,
            "canary",
        ))
        .region(Region::new("us-east-1"))
        .http_client(http_client.clone())
        .build();
    let client = aws_sdk_s3::Client::from_conf(config);

    let created = client
        .create_multipart_upload()
        .bucket("canary-bucket")
        .key("canary-key")
        .send()
        .await
        .expect("create_multipart_upload");
    let upload_id = created
        .upload_id()
        .expect("no upload id returned")
        .to_string();
    assert_eq!(upload_id, "UPLOAD-ID-1");

    let mut parts = Vec::new();
    for part_number in 1..=2 {
        let part = client
            .upload_part()
            .bucket("canary-bucket")
            .key("canary-key")
            .upload_id(&upload_id)
            .part_number(part_number)
            .body(aws_sdk_s3::primitives::ByteStream::from_static(
                b"canary-part-body",
            ))
            .send()
            .await
            .unwrap_or_else(|e| panic!("upload_part {part_number} failed: {e:?}"));
        parts.push(
            aws_sdk_s3::types::CompletedPart::builder()
                .part_number(part_number)
                .e_tag(part.e_tag().expect("part returned no ETag"))
                .build(),
        );
    }
    assert_eq!(
        parts.iter().filter_map(|p| p.e_tag()).collect::<Vec<_>>(),
        vec!["\"etag-part-1\"", "\"etag-part-2\""],
        "part ETags were not deserialized from the response headers"
    );

    let completed = client
        .complete_multipart_upload()
        .bucket("canary-bucket")
        .key("canary-key")
        .upload_id(&upload_id)
        .multipart_upload(
            aws_sdk_s3::types::CompletedMultipartUpload::builder()
                .set_parts(Some(parts))
                .build(),
        )
        .send()
        .await
        .expect("complete_multipart_upload");
    assert_eq!(completed.e_tag(), Some("\"etag-final\""));

    // Four requests, and the upload id must be on every one after create.
    let sent: Vec<_> = http_client.actual_requests().collect();
    assert_eq!(sent.len(), 4, "expected 4 requests, got {}", sent.len());
    for (i, r) in sent.iter().enumerate().skip(1) {
        assert!(
            r.uri().contains("uploadId=UPLOAD-ID-1"),
            "request {i} did not carry the upload id: {}",
            r.uri()
        );
    }
    assert!(
        sent[1].uri().contains("partNumber=1") && sent[2].uri().contains("partNumber=2"),
        "part numbers not plumbed: {} / {}",
        sent[1].uri(),
        sent[2].uri()
    );
}
