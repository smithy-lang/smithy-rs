/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! HTTP/1.1 connection behavior contracts for Smithy HTTP client implementations.
//!
//! Each contract has an implementation-neutral scenario followed by an explicit test runner for
//! every backend that must satisfy it.

#![cfg(all(feature = "wire-mock", feature = "default-client"))]

mod common;

use aws_smithy_http_client::test_util::wire::connection::{
    BodyPlan, ConnectionCloseReason, ConnectionEvent, ConnectionId, ConnectionScript,
    ConnectionTestHarness, EndpointPlan, Http1Response, Http1Script, ManualGate, SocketScript,
};
use aws_smithy_runtime_api::client::connection::{
    CaptureSmithyConnection, ConnectionMetadata as SmithyConnectionMetadata,
};
use aws_smithy_runtime_api::client::http::{
    HttpClient, HttpConnectorSettings, SharedHttpConnector,
};
use aws_smithy_runtime_api::client::orchestrator::HttpRequest;
use aws_smithy_types::body::SdkBody;
use aws_smithy_types::retry::ErrorKind;
use common::client as test_client;
use common::client::{BackendConfig, HttpClientBackend, HyperUtilLegacyPool};
use http_body_util::BodyExt;
use std::borrow::Cow;
use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

const IP1: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

fn request_with_body(url: &str, body: &[u8]) -> HttpRequest {
    let mut request = HttpRequest::new(SdkBody::from(body.to_vec()));
    request.set_method("POST").expect("valid HTTP method");
    request.set_uri(url).expect("valid HTTP URI");
    request
        .headers_mut()
        .insert("content-length", body.len().to_string());
    request
}

async fn get_and_collect_with_capture(
    connector: &SharedHttpConnector,
    url: &str,
) -> (u16, Vec<u8>, SmithyConnectionMetadata) {
    let capture = CaptureSmithyConnection::new();
    let mut request = HttpRequest::get(url).expect("valid HTTP request");
    request.add_extension(capture.clone());
    let (status, body) = test_client::send_and_collect(connector, request).await;
    let metadata = capture
        .get()
        .expect("CaptureSmithyConnection should contain connection metadata");
    (status, body, metadata)
}

fn http1_request_connection_ids(harness: &ConnectionTestHarness) -> Vec<ConnectionId> {
    harness
        .events()
        .into_iter()
        .filter_map(|event| match event {
            ConnectionEvent::Http1Request { connection_id, .. } => Some(connection_id),
            _ => None,
        })
        .collect()
}

mod reuse_and_lifecycle {
    use super::*;

    async fn fully_consumed_responses_reuse_connection(backend: &dyn HttpClientBackend) {
        let harness = ConnectionTestHarness::builder()
            .endpoint(
                IP1,
                Http1Script::responses([
                    Http1Response::ok().body("first"),
                    Http1Response::ok().body("second"),
                    Http1Response::ok().body("third"),
                ]),
            )
            .build()
            .await
            .expect("harness should start");
        let client = backend.build(BackendConfig::default());
        let connector = test_client::default_connector(&client);

        for expected in [b"first".as_slice(), b"second", b"third"] {
            let (status, body) =
                test_client::get_and_collect(&connector, &harness.endpoint_url()).await;
            assert_eq!(status, 200);
            assert_eq!(body, expected);
        }

        let connection_ids = http1_request_connection_ids(&harness);
        assert_eq!(connection_ids.len(), 3);
        assert!(
            connection_ids
                .iter()
                .all(|connection_id| *connection_id == connection_ids[0]),
            "fully consumed responses should reuse one connection"
        );
        assert_eq!(harness.tcp_accepted_count(), 1);

        drop(connector);
        drop(client);
        harness.shutdown().await.expect("clean harness shutdown");
    }

    #[tokio::test]
    async fn test_fully_consumed_responses_reuse_connection_with_hyper_util_legacy_pool() {
        fully_consumed_responses_reuse_connection(&HyperUtilLegacyPool).await;
    }

    async fn idle_connection_is_evicted_after_timeout(backend: &dyn HttpClientBackend) {
        let idle_timeout = Duration::from_millis(100);
        let harness = ConnectionTestHarness::builder()
            .endpoint(
                IP1,
                EndpointPlan::queue([
                    Http1Script::responses([Http1Response::ok().body("first")]),
                    Http1Script::responses([Http1Response::ok().body("second")]),
                ]),
            )
            .build()
            .await
            .expect("harness should start");
        let client = backend.build(BackendConfig {
            pool_idle_timeout: Some(idle_timeout),
        });
        let connector = test_client::default_connector(&client);

        let (status, body) =
            test_client::get_and_collect(&connector, &harness.endpoint_url()).await;
        assert_eq!((status, body.as_slice()), (200, b"first".as_slice()));
        let first_connection = http1_request_connection_ids(&harness)[0];

        harness
            .wait_for_event(test_client::WAIT, |event| {
                matches!(
                    event,
                    ConnectionEvent::ConnectionClosed {
                        connection_id,
                        reason: ConnectionCloseReason::ClientClosed,
                    } if *connection_id == first_connection
                )
            })
            .await
            .expect("the client should close the evicted idle connection");

        let (status, body) =
            test_client::get_and_collect(&connector, &harness.endpoint_url()).await;
        assert_eq!((status, body.as_slice()), (200, b"second".as_slice()));

        let connection_ids = http1_request_connection_ids(&harness);
        assert_eq!(connection_ids.len(), 2);
        assert_ne!(
            connection_ids[0], connection_ids[1],
            "a connection evicted by the idle timeout must not be reused"
        );

        drop(connector);
        drop(client);
        harness.shutdown().await.expect("clean harness shutdown");
    }

    #[tokio::test]
    async fn test_idle_connection_is_evicted_after_timeout_with_hyper_util_legacy_pool() {
        idle_connection_is_evicted_after_timeout(&HyperUtilLegacyPool).await;
    }

    async fn active_response_body_survives_idle_timeout(backend: &dyn HttpClientBackend) {
        let idle_timeout = Duration::from_millis(100);
        let body_gate = ManualGate::new();
        let harness = ConnectionTestHarness::builder()
            .endpoint(
                IP1,
                Http1Script::responses([
                    Http1Response::ok().body_plan(BodyPlan::split_at_gate(
                        "first-",
                        body_gate.waiter(),
                        "body",
                    )),
                    Http1Response::ok().body("second"),
                ]),
            )
            .build()
            .await
            .expect("harness should start");
        let client = backend.build(BackendConfig {
            pool_idle_timeout: Some(idle_timeout),
        });
        let connector = test_client::default_connector(&client);

        let first_response = test_client::send_request(
            &connector,
            HttpRequest::get(harness.endpoint_url()).expect("valid HTTP request"),
        )
        .await
        .expect("first request should return response headers");
        body_gate
            .wait_until_reached(test_client::WAIT)
            .await
            .expect("the first response should reach its body gate");

        tokio::time::sleep(idle_timeout * 3).await;
        body_gate.release();
        let (status, body) = test_client::collect_response(first_response).await;
        assert_eq!((status, body.as_slice()), (200, b"first-body".as_slice()));

        let (status, body) =
            test_client::get_and_collect(&connector, &harness.endpoint_url()).await;
        assert_eq!((status, body.as_slice()), (200, b"second".as_slice()));

        let connection_ids = http1_request_connection_ids(&harness);
        assert_eq!(connection_ids.len(), 2);
        assert_eq!(
            connection_ids[0], connection_ids[1],
            "an active response body must survive the pool idle timeout"
        );
        assert_eq!(harness.tcp_accepted_count(), 1);

        drop(connector);
        drop(client);
        harness.shutdown().await.expect("clean harness shutdown");
    }

    #[tokio::test]
    async fn test_active_response_body_survives_idle_timeout_with_hyper_util_legacy_pool() {
        active_response_body_survives_idle_timeout(&HyperUtilLegacyPool).await;
    }

    async fn held_response_body_allows_second_connection(backend: &dyn HttpClientBackend) {
        let body_gate = ManualGate::new();
        let harness = ConnectionTestHarness::builder()
            .endpoint(
                IP1,
                EndpointPlan::queue([
                    Http1Script::responses([Http1Response::ok()
                        .body_plan(BodyPlan::split_at_gate("held-", body_gate.waiter(), "body"))]),
                    Http1Script::responses([Http1Response::ok().body("second")]),
                ]),
            )
            .build()
            .await
            .expect("harness should start");
        let client = backend.build(BackendConfig::default());
        let connector = test_client::default_connector(&client);

        let first_response = test_client::send_request(
            &connector,
            HttpRequest::get(harness.endpoint_url()).expect("valid HTTP request"),
        )
        .await
        .expect("first request should return response headers");
        body_gate
            .wait_until_reached(test_client::WAIT)
            .await
            .expect("the first response should reach its body gate");

        let (status, body) =
            test_client::get_and_collect(&connector, &harness.endpoint_url()).await;
        assert_eq!((status, body.as_slice()), (200, b"second".as_slice()));

        let connection_ids = http1_request_connection_ids(&harness);
        assert_eq!(connection_ids.len(), 2);
        assert_ne!(
            connection_ids[0], connection_ids[1],
            "a held H1 response body should make a second connection available"
        );

        body_gate.release();
        let (status, body) = test_client::collect_response(first_response).await;
        assert_eq!((status, body.as_slice()), (200, b"held-body".as_slice()));

        drop(connector);
        drop(client);
        harness.shutdown().await.expect("clean harness shutdown");
    }

    #[tokio::test]
    async fn test_held_response_body_allows_second_connection_with_hyper_util_legacy_pool() {
        held_response_body_allows_second_connection(&HyperUtilLegacyPool).await;
    }

    async fn dropping_buffered_chunk_terminator_allows_reuse(backend: &dyn HttpClientBackend) {
        let harness = ConnectionTestHarness::builder()
            .endpoint(
                IP1,
                EndpointPlan::queue([
                    SocketScript::new()
                        .read_http1_request()
                        .write_all(
                            b"HTTP/1.1 200 OK\r\n\
                              Transfer-Encoding: chunked\r\n\
                              Connection: keep-alive\r\n\
                              \r\n\
                              5\r\nfirst\r\n0\r\n\r\n",
                        )
                        .read_http1_request()
                        .write_all(
                            b"HTTP/1.1 200 OK\r\n\
                              Content-Length: 6\r\n\
                              Connection: keep-alive\r\n\
                              \r\n\
                              second",
                        )
                        .await_client_close(),
                    SocketScript::new().await_client_close(),
                ]),
            )
            .build()
            .await
            .expect("harness should start");
        let client = backend.build(BackendConfig::default());
        let connector = test_client::default_connector(&client);

        let mut first_response = test_client::send_request(
            &connector,
            HttpRequest::get(harness.endpoint_url()).expect("valid HTTP request"),
        )
        .await
        .expect("first request should succeed");
        assert_eq!(first_response.status().as_u16(), 200);
        let frame = first_response
            .body_mut()
            .frame()
            .await
            .expect("first response should contain a data frame")
            .expect("first response frame should be readable");
        assert_eq!(
            frame
                .into_data()
                .expect("first response frame should contain data"),
            b"first".as_slice()
        );
        drop(first_response);

        let (status, body) =
            test_client::get_and_collect(&connector, &harness.endpoint_url()).await;
        assert_eq!((status, body.as_slice()), (200, b"second".as_slice()));

        let connection_ids = http1_request_connection_ids(&harness);
        assert_eq!(connection_ids.len(), 2);
        assert_eq!(
            connection_ids[0], connection_ids[1],
            "Hyper should drain the buffered chunk terminator and reuse the connection"
        );

        drop(connector);
        drop(client);
        harness.shutdown().await.expect("clean harness shutdown");
    }

    #[tokio::test]
    async fn test_dropping_buffered_chunk_terminator_allows_reuse_with_hyper_util_legacy_pool() {
        dropping_buffered_chunk_terminator_allows_reuse(&HyperUtilLegacyPool).await;
    }

    async fn dropping_unavailable_response_remainder_retires_connection(
        backend: &dyn HttpClientBackend,
    ) {
        let harness = ConnectionTestHarness::builder()
            .endpoint(
                IP1,
                EndpointPlan::queue([
                    ConnectionScript::socket(
                        SocketScript::new()
                            .read_http1_request()
                            .write_all(
                                b"HTTP/1.1 200 OK\r\n\
                                  Content-Length: 10\r\n\
                                  Connection: keep-alive\r\n\
                                  \r\n\
                                  first",
                            )
                            .await_client_close(),
                    ),
                    ConnectionScript::http1(Http1Script::responses([
                        Http1Response::ok().body("second")
                    ])),
                ]),
            )
            .build()
            .await
            .expect("harness should start");
        let client = backend.build(BackendConfig::default());
        let connector = test_client::default_connector(&client);

        let mut first_response = test_client::send_request(
            &connector,
            HttpRequest::get(harness.endpoint_url()).expect("valid HTTP request"),
        )
        .await
        .expect("first request should return response headers");
        assert_eq!(first_response.status().as_u16(), 200);
        let frame = first_response
            .body_mut()
            .frame()
            .await
            .expect("first response should contain a partial data frame")
            .expect("first response frame should be readable");
        assert_eq!(
            frame
                .into_data()
                .expect("first response frame should contain data"),
            b"first".as_slice()
        );
        let first_connection = http1_request_connection_ids(&harness)[0];
        drop(first_response);

        harness
            .wait_for_event(test_client::WAIT, |event| {
                matches!(
                    event,
                    ConnectionEvent::ConnectionClosed {
                        connection_id,
                        reason: ConnectionCloseReason::ClientClosed,
                    } if *connection_id == first_connection
                )
            })
            .await
            .expect("dropping the incomplete body should close the connection");

        let (status, body) =
            test_client::get_and_collect(&connector, &harness.endpoint_url()).await;
        assert_eq!((status, body.as_slice()), (200, b"second".as_slice()));

        let connection_ids = http1_request_connection_ids(&harness);
        assert_eq!(connection_ids.len(), 2);
        assert_ne!(
            connection_ids[0], connection_ids[1],
            "a connection with an unavailable response remainder must be replaced"
        );

        drop(connector);
        drop(client);
        harness.shutdown().await.expect("clean harness shutdown");
    }

    #[tokio::test]
    async fn test_dropping_unavailable_response_remainder_retires_connection_with_hyper_util_legacy_pool(
    ) {
        dropping_unavailable_response_remainder_retires_connection(&HyperUtilLegacyPool).await;
    }

    async fn stale_idle_connection_is_replaced(backend: &dyn HttpClientBackend) {
        let close_gate = ManualGate::new();
        let harness = ConnectionTestHarness::builder()
            .endpoint(
                IP1,
                EndpointPlan::queue([
                    ConnectionScript::socket(
                        SocketScript::new()
                            .read_http1_request()
                            .write_all(
                                b"HTTP/1.1 200 OK\r\n\
                                  Content-Length: 5\r\n\
                                  Connection: keep-alive\r\n\
                                  \r\n\
                                  first",
                            )
                            .wait(close_gate.waiter())
                            .close(),
                    ),
                    ConnectionScript::http1(Http1Script::responses([
                        Http1Response::ok().body("second")
                    ])),
                ]),
            )
            .build()
            .await
            .expect("harness should start");
        let client = backend.build(BackendConfig::default());
        let connector = test_client::default_connector(&client);

        let (status, body) =
            test_client::get_and_collect(&connector, &harness.endpoint_url()).await;
        assert_eq!((status, body.as_slice()), (200, b"first".as_slice()));
        close_gate
            .wait_until_reached(test_client::WAIT)
            .await
            .expect("the server should be ready to close the idle connection");
        let first_connection = http1_request_connection_ids(&harness)[0];

        close_gate.release();
        harness
            .wait_for_event(test_client::WAIT, |event| {
                matches!(
                    event,
                    ConnectionEvent::ConnectionClosed {
                        connection_id,
                        reason: ConnectionCloseReason::ScriptCompleted,
                    } if *connection_id == first_connection
                )
            })
            .await
            .expect("the server should close the first connection");

        let (status, body) =
            test_client::get_and_collect(&connector, &harness.endpoint_url()).await;
        assert_eq!((status, body.as_slice()), (200, b"second".as_slice()));

        let connection_ids = http1_request_connection_ids(&harness);
        assert_eq!(connection_ids.len(), 2);
        assert_ne!(
            connection_ids[0], connection_ids[1],
            "a server-closed idle connection must be replaced"
        );

        drop(connector);
        drop(client);
        harness.shutdown().await.expect("clean harness shutdown");
    }

    #[tokio::test]
    async fn test_stale_idle_connection_is_replaced_with_hyper_util_legacy_pool() {
        stale_idle_connection_is_replaced(&HyperUtilLegacyPool).await;
    }

    async fn connection_close_response_is_not_reused(backend: &dyn HttpClientBackend) {
        let harness = ConnectionTestHarness::builder()
            .endpoint(
                IP1,
                EndpointPlan::queue([
                    ConnectionScript::socket(
                        SocketScript::new()
                            .read_http1_request()
                            .write_all(
                                b"HTTP/1.1 200 OK\r\n\
                                  Content-Length: 7\r\n\
                                  Connection: close\r\n\
                                  \r\n\
                                  closing",
                            )
                            .await_client_close(),
                    ),
                    ConnectionScript::http1(Http1Script::responses([
                        Http1Response::ok().body("fresh")
                    ])),
                ]),
            )
            .build()
            .await
            .expect("harness should start");
        let client = backend.build(BackendConfig::default());
        let connector = test_client::default_connector(&client);

        let (status, body) =
            test_client::get_and_collect(&connector, &harness.endpoint_url()).await;
        assert_eq!((status, body.as_slice()), (200, b"closing".as_slice()));
        let first_connection = http1_request_connection_ids(&harness)[0];
        harness
            .wait_for_event(test_client::WAIT, |event| {
                matches!(
                    event,
                    ConnectionEvent::ConnectionClosed {
                        connection_id,
                        reason: ConnectionCloseReason::ClientClosed,
                    } if *connection_id == first_connection
                )
            })
            .await
            .expect("the client should close a connection marked Connection: close");

        let (status, body) =
            test_client::get_and_collect(&connector, &harness.endpoint_url()).await;
        assert_eq!((status, body.as_slice()), (200, b"fresh".as_slice()));

        let connection_ids = http1_request_connection_ids(&harness);
        assert_eq!(connection_ids.len(), 2);
        assert_ne!(
            connection_ids[0], connection_ids[1],
            "a Connection: close response must not be reused"
        );
        assert_eq!(harness.tcp_accepted_count(), 2);

        drop(connector);
        drop(client);
        harness.shutdown().await.expect("clean harness shutdown");
    }

    #[tokio::test]
    async fn test_connection_close_response_is_not_reused_with_hyper_util_legacy_pool() {
        connection_close_response_is_not_reused(&HyperUtilLegacyPool).await;
    }
}

mod routing_and_status {
    use super::*;

    async fn direct_request_uses_origin_form_and_host_header(backend: &dyn HttpClientBackend) {
        let harness = ConnectionTestHarness::builder()
            .endpoint(
                IP1,
                Http1Script::responses([Http1Response::ok().body("ok")]),
            )
            .build()
            .await
            .expect("harness should start");
        let client = backend.build(BackendConfig::default());
        let connector = test_client::default_connector(&client);
        let url = format!(
            "{}/some/path?key=value",
            harness.endpoint_url().trim_end_matches('/')
        );

        let (status, body) = test_client::get_and_collect(&connector, &url).await;
        assert_eq!((status, body.as_slice()), (200, b"ok".as_slice()));

        let requests = harness.http_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].0, "/some/path?key=value");
        let expected_host = format!("127.0.0.1:{}", harness.port());
        assert_eq!(requests[0].1.as_deref(), Some(expected_host.as_str()));

        drop(connector);
        drop(client);
        harness.shutdown().await.expect("clean harness shutdown");
    }

    #[tokio::test]
    async fn test_direct_request_uses_origin_form_and_host_header_with_hyper_util_legacy_pool() {
        direct_request_uses_origin_form_and_host_header(&HyperUtilLegacyPool).await;
    }

    async fn different_origins_do_not_share_connections(backend: &dyn HttpClientBackend) {
        let harness = ConnectionTestHarness::builder()
            .endpoint(
                IP1,
                EndpointPlan::queue([
                    Http1Script::responses([
                        Http1Response::ok().body("ip-a"),
                        Http1Response::ok().body("ip-b"),
                    ]),
                    Http1Script::responses([
                        Http1Response::ok().body("localhost-a"),
                        Http1Response::ok().body("localhost-b"),
                    ]),
                ]),
            )
            .build()
            .await
            .expect("harness should start");
        let client = backend.build(BackendConfig::default());
        let connector = test_client::default_connector(&client);
        let ip_url = harness.endpoint_url();
        let localhost_url = format!("http://localhost:{}/", harness.port());

        for (url, expected) in [
            (&ip_url, b"ip-a".as_slice()),
            (&localhost_url, b"localhost-a".as_slice()),
            (&ip_url, b"ip-b".as_slice()),
            (&localhost_url, b"localhost-b".as_slice()),
        ] {
            let (status, body) = test_client::get_and_collect(&connector, url).await;
            assert_eq!(status, 200);
            assert_eq!(body, expected);
        }

        let requests = harness
            .events()
            .into_iter()
            .filter_map(|event| match event {
                ConnectionEvent::Http1Request {
                    connection_id,
                    host,
                    ..
                } => Some((connection_id, host)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(requests.len(), 4);
        assert_eq!(requests[0].1, Some(format!("127.0.0.1:{}", harness.port())));
        assert_eq!(requests[1].1, Some(format!("localhost:{}", harness.port())));
        assert_eq!(requests[2].1, requests[0].1);
        assert_eq!(requests[3].1, requests[1].1);
        assert_eq!(requests[0].0, requests[2].0);
        assert_eq!(requests[1].0, requests[3].0);
        assert_ne!(
            requests[0].0, requests[1].0,
            "distinct authorities resolving to one endpoint must not share an H1 connection"
        );
        assert_eq!(harness.tcp_accepted_count(), 2);

        drop(connector);
        drop(client);
        harness.shutdown().await.expect("clean harness shutdown");
    }

    #[tokio::test]
    async fn test_different_origins_do_not_share_connections_with_hyper_util_legacy_pool() {
        different_origins_do_not_share_connections(&HyperUtilLegacyPool).await;
    }

    async fn raw_server_error_response_does_not_poison_connection(backend: &dyn HttpClientBackend) {
        let harness = ConnectionTestHarness::builder()
            .endpoint(
                IP1,
                Http1Script::responses([
                    Http1Response::new(503).body("unavailable"),
                    Http1Response::ok().body("recovered"),
                ]),
            )
            .build()
            .await
            .expect("harness should start");
        let client = backend.build(BackendConfig::default());
        let connector = test_client::default_connector(&client);

        let (status, body) =
            test_client::get_and_collect(&connector, &harness.endpoint_url()).await;
        assert_eq!((status, body.as_slice()), (503, b"unavailable".as_slice()));

        let (status, body) =
            test_client::get_and_collect(&connector, &harness.endpoint_url()).await;
        assert_eq!((status, body.as_slice()), (200, b"recovered".as_slice()));

        let connection_ids = http1_request_connection_ids(&harness);
        assert_eq!(connection_ids.len(), 2);
        assert_eq!(
            connection_ids[0], connection_ids[1],
            "a raw server error response must not poison the connection"
        );
        assert_eq!(harness.tcp_accepted_count(), 1);

        drop(connector);
        drop(client);
        harness.shutdown().await.expect("clean harness shutdown");
    }

    #[tokio::test]
    async fn test_raw_server_error_response_does_not_poison_connection_with_hyper_util_legacy_pool()
    {
        raw_server_error_response_does_not_poison_connection(&HyperUtilLegacyPool).await;
    }
}

mod connection_metadata {
    use super::*;

    async fn captured_connection_addresses_and_poison_prevent_reuse(
        backend: &dyn HttpClientBackend,
    ) {
        let harness = ConnectionTestHarness::builder()
            .endpoint(
                IP1,
                EndpointPlan::queue([
                    Http1Script::responses([Http1Response::ok().body("first")]),
                    Http1Script::responses([Http1Response::ok().body("second")]),
                ]),
            )
            .build()
            .await
            .expect("harness should start");
        let client = backend.build(BackendConfig::default());
        let connector = test_client::default_connector(&client);

        let (status, body, metadata) =
            get_and_collect_with_capture(&connector, &harness.endpoint_url()).await;
        assert_eq!((status, body.as_slice()), (200, b"first".as_slice()));
        assert_eq!(
            metadata.remote_addr(),
            Some(harness.endpoint(0).expect("first endpoint").addr())
        );
        let local_addr = metadata
            .local_addr()
            .expect("direct connection should include its local address");
        assert_eq!(local_addr.ip(), IP1);
        assert_ne!(local_addr.port(), 0);

        metadata.poison();

        let (status, body) =
            test_client::get_and_collect(&connector, &harness.endpoint_url()).await;
        assert_eq!((status, body.as_slice()), (200, b"second".as_slice()));

        let connection_ids = http1_request_connection_ids(&harness);
        assert_eq!(connection_ids.len(), 2);
        assert_ne!(
            connection_ids[0], connection_ids[1],
            "poisoned connection metadata must prevent connection reuse"
        );

        drop(connector);
        drop(client);
        harness.shutdown().await.expect("clean harness shutdown");
    }

    #[tokio::test]
    async fn test_captured_connection_addresses_and_poison_prevent_reuse_with_hyper_util_legacy_pool(
    ) {
        captured_connection_addresses_and_poison_prevent_reuse(&HyperUtilLegacyPool).await;
    }

    async fn poisoning_active_connection_allows_body_completion_and_prevents_reuse(
        backend: &dyn HttpClientBackend,
    ) {
        let body_gate = ManualGate::new();
        let harness = ConnectionTestHarness::builder()
            .endpoint(
                IP1,
                EndpointPlan::queue([
                    Http1Script::responses([Http1Response::ok().body_plan(
                        BodyPlan::split_at_gate("first-", body_gate.waiter(), "body"),
                    )]),
                    Http1Script::responses([Http1Response::ok().body("second")]),
                ]),
            )
            .build()
            .await
            .expect("harness should start");
        let client = backend.build(BackendConfig::default());
        let connector = test_client::default_connector(&client);
        let capture = CaptureSmithyConnection::new();
        let mut request = HttpRequest::get(harness.endpoint_url()).expect("valid HTTP request");
        request.add_extension(capture.clone());

        let first_response = test_client::send_request(&connector, request)
            .await
            .expect("first request should return response headers");
        body_gate
            .wait_until_reached(test_client::WAIT)
            .await
            .expect("the active response should reach its body gate");
        let first_connection = http1_request_connection_ids(&harness)[0];
        capture
            .get()
            .expect("active request should expose connection metadata")
            .poison();

        body_gate.release();
        let (status, body) = test_client::collect_response(first_response).await;
        assert_eq!((status, body.as_slice()), (200, b"first-body".as_slice()));
        harness
            .wait_for_event(test_client::WAIT, |event| {
                matches!(
                    event,
                    ConnectionEvent::ConnectionClosed {
                        connection_id,
                        reason: ConnectionCloseReason::ClientClosed,
                    } if *connection_id == first_connection
                )
            })
            .await
            .expect("the poisoned connection should retire after its active body completes");

        let (status, body) =
            test_client::get_and_collect(&connector, &harness.endpoint_url()).await;
        assert_eq!((status, body.as_slice()), (200, b"second".as_slice()));
        let connection_ids = http1_request_connection_ids(&harness);
        assert_eq!(connection_ids.len(), 2);
        assert_ne!(
            connection_ids[0], connection_ids[1],
            "a connection poisoned while active must not be reused"
        );

        drop(connector);
        drop(client);
        harness.shutdown().await.expect("clean harness shutdown");
    }

    #[tokio::test]
    async fn test_poisoning_active_connection_allows_body_completion_and_prevents_reuse_with_hyper_util_legacy_pool(
    ) {
        poisoning_active_connection_allows_body_completion_and_prevents_reuse(&HyperUtilLegacyPool)
            .await;
    }

    async fn captured_connection_without_poison_is_reused(backend: &dyn HttpClientBackend) {
        let harness = ConnectionTestHarness::builder()
            .endpoint(
                IP1,
                Http1Script::responses([
                    Http1Response::ok().body("first"),
                    Http1Response::ok().body("second"),
                ]),
            )
            .build()
            .await
            .expect("harness should start");
        let client = backend.build(BackendConfig::default());
        let connector = test_client::default_connector(&client);

        let (status, body, metadata) =
            get_and_collect_with_capture(&connector, &harness.endpoint_url()).await;
        assert_eq!((status, body.as_slice()), (200, b"first".as_slice()));
        drop(metadata);

        let (status, body) =
            test_client::get_and_collect(&connector, &harness.endpoint_url()).await;
        assert_eq!((status, body.as_slice()), (200, b"second".as_slice()));

        let connection_ids = http1_request_connection_ids(&harness);
        assert_eq!(connection_ids.len(), 2);
        assert_eq!(
            connection_ids[0], connection_ids[1],
            "capturing metadata without poisoning should permit reuse"
        );
        assert_eq!(harness.tcp_accepted_count(), 1);

        drop(connector);
        drop(client);
        harness.shutdown().await.expect("clean harness shutdown");
    }

    #[tokio::test]
    async fn test_captured_connection_without_poison_is_reused_with_hyper_util_legacy_pool() {
        captured_connection_without_poison_is_reused(&HyperUtilLegacyPool).await;
    }

    fn connector_metadata_identifies_hyper_1x(backend: &dyn HttpClientBackend) {
        let client = backend.build(BackendConfig::default());
        let metadata = client
            .connector_metadata()
            .expect("connector metadata should be present");

        assert_eq!(metadata.name(), Cow::Borrowed("hyper"));
        assert_eq!(metadata.version(), Some(Cow::Borrowed("1.x")));
    }

    #[test]
    fn test_connector_metadata_identifies_hyper_1x_with_hyper_util_legacy_pool() {
        connector_metadata_identifies_hyper_1x(&HyperUtilLegacyPool);
    }
}

mod failures_and_timeouts {
    use super::*;

    async fn reset_on_accept_is_io_error(backend: &dyn HttpClientBackend) {
        let harness = ConnectionTestHarness::builder()
            .endpoint(IP1, SocketScript::new().reset())
            .build()
            .await
            .expect("harness should start");
        let client = backend.build(BackendConfig::default());
        let connector = test_client::default_connector(&client);

        let error = test_client::send_request(
            &connector,
            HttpRequest::get(harness.endpoint_url()).expect("valid HTTP request"),
        )
        .await
        .expect_err("reset on accept should fail the request");
        assert!(error.is_io(), "expected ConnectorError::io, got {error:?}");

        drop(connector);
        drop(client);
        harness.shutdown().await.expect("clean harness shutdown");
    }

    #[tokio::test]
    async fn test_reset_on_accept_is_io_error_with_hyper_util_legacy_pool() {
        reset_on_accept_is_io_error(&HyperUtilLegacyPool).await;
    }

    async fn reset_after_complete_request_is_io_error(backend: &dyn HttpClientBackend) {
        let harness = ConnectionTestHarness::builder()
            .endpoint(IP1, SocketScript::new().read_http1_request().reset())
            .build()
            .await
            .expect("harness should start");
        let client = backend.build(BackendConfig::default());
        let connector = test_client::default_connector(&client);
        let request_body = b"complete request body";

        let error = test_client::send_request(
            &connector,
            request_with_body(&harness.endpoint_url(), request_body),
        )
        .await
        .expect_err("reset after the request should fail before response headers");
        assert!(error.is_io(), "expected ConnectorError::io, got {error:?}");

        let requests = harness.events();
        assert!(
            requests.iter().any(|event| matches!(
                event,
                ConnectionEvent::Http1Request { method, .. } if method == "POST"
            )),
            "the harness must receive the complete framed request before resetting"
        );

        drop(connector);
        drop(client);
        harness.shutdown().await.expect("clean harness shutdown");
    }

    #[tokio::test]
    async fn test_reset_after_complete_request_is_io_error_with_hyper_util_legacy_pool() {
        reset_after_complete_request_is_io_error(&HyperUtilLegacyPool).await;
    }

    async fn reset_during_response_body_fails_body_only(backend: &dyn HttpClientBackend) {
        let reset_gate = ManualGate::new();
        let harness = ConnectionTestHarness::builder()
            .endpoint(
                IP1,
                SocketScript::new()
                    .read_http1_request()
                    .write_all(
                        b"HTTP/1.1 200 OK\r\n\
                          Content-Length: 10\r\n\
                          Connection: keep-alive\r\n\
                          \r\n\
                          first",
                    )
                    .wait(reset_gate.waiter())
                    .reset(),
            )
            .build()
            .await
            .expect("harness should start");
        let client = backend.build(BackendConfig::default());
        let connector = test_client::default_connector(&client);

        let mut response = test_client::send_request(
            &connector,
            HttpRequest::get(harness.endpoint_url()).expect("valid HTTP request"),
        )
        .await
        .expect("response headers should complete before the reset");
        assert_eq!(response.status().as_u16(), 200);
        reset_gate
            .wait_until_reached(test_client::WAIT)
            .await
            .expect("the response should reach its reset gate");
        let frame = response
            .body_mut()
            .frame()
            .await
            .expect("response should contain a partial data frame")
            .expect("partial response frame should be readable");
        assert_eq!(
            frame
                .into_data()
                .expect("partial response frame should contain data"),
            b"first".as_slice()
        );

        reset_gate.release();
        tokio::time::timeout(test_client::WAIT, response.into_body().collect())
            .await
            .expect("response body should fail within the outer deadline")
            .expect_err("reset should fail collection of the remaining response body");

        drop(connector);
        drop(client);
        harness.shutdown().await.expect("clean harness shutdown");
    }

    #[tokio::test]
    async fn test_reset_during_response_body_fails_body_only_with_hyper_util_legacy_pool() {
        reset_during_response_body_fails_body_only(&HyperUtilLegacyPool).await;
    }

    async fn clean_eof_before_response_is_transient_other(backend: &dyn HttpClientBackend) {
        let harness = ConnectionTestHarness::builder()
            .endpoint(
                IP1,
                SocketScript::new()
                    .read_http1_request()
                    .shutdown_write()
                    .await_client_close(),
            )
            .build()
            .await
            .expect("harness should start");
        let client = backend.build(BackendConfig::default());
        let connector = test_client::default_connector(&client);

        let error = test_client::send_request(
            &connector,
            HttpRequest::get(harness.endpoint_url()).expect("valid HTTP request"),
        )
        .await
        .expect_err("clean EOF before response headers should fail the request");
        assert!(
            error.is_other(),
            "expected ConnectorError::other, got {error:?}"
        );
        assert_eq!(error.as_other(), Some(ErrorKind::TransientError));

        drop(connector);
        drop(client);
        harness.shutdown().await.expect("clean harness shutdown");
    }

    #[tokio::test]
    async fn test_clean_eof_before_response_is_transient_other_with_hyper_util_legacy_pool() {
        clean_eof_before_response_is_transient_other(&HyperUtilLegacyPool).await;
    }

    async fn read_timeout_is_timeout_error(backend: &dyn HttpClientBackend) {
        let read_timeout = Duration::from_millis(250);
        let silent_gate = ManualGate::new();
        let harness = ConnectionTestHarness::builder()
            .endpoint(
                IP1,
                SocketScript::new()
                    .read_http1_request()
                    .wait(silent_gate.waiter()),
            )
            .build()
            .await
            .expect("harness should start");
        let client = backend.build(BackendConfig::default());
        let connector = test_client::connector(
            &client,
            HttpConnectorSettings::builder()
                .read_timeout(read_timeout)
                .build(),
        );
        let url = harness.endpoint_url();
        let request_task = tokio::spawn({
            let connector = connector.clone();
            async move {
                test_client::send_request(
                    &connector,
                    HttpRequest::get(url).expect("valid HTTP request"),
                )
                .await
            }
        });

        silent_gate
            .wait_until_reached(test_client::WAIT)
            .await
            .expect("the server should receive the request and remain silent");
        let error = tokio::time::timeout(test_client::WAIT, request_task)
            .await
            .expect("request should finish within the outer deadline")
            .expect("request task should not panic")
            .expect_err("the read timeout should fail the request");
        assert!(
            error.is_timeout(),
            "expected ConnectorError::timeout, got {error:?}"
        );

        silent_gate.release();
        drop(connector);
        drop(client);
        harness.shutdown().await.expect("clean harness shutdown");
    }

    #[tokio::test]
    async fn test_read_timeout_is_timeout_error_with_hyper_util_legacy_pool() {
        read_timeout_is_timeout_error(&HyperUtilLegacyPool).await;
    }
}
