/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Behavior specific to the partition-aware connection pool.

#![cfg(all(
    feature = "wire-mock",
    feature = "default-client",
    feature = "rt-tokio"
))]

mod common {
    #[allow(dead_code)]
    pub(crate) mod client;
}

use aws_smithy_http_client::pool::{
    Client, ConnectionPool, ConnectionReuseScope, Partition, PartitionId, TokioDriverSpawner,
};
use aws_smithy_http_client::test_util::wire::connection::{
    BodyPlan, ConnectionCloseReason, ConnectionEvent, ConnectionTestHarness, EndpointPlan,
    Http1Response, Http1Script, ManualGate,
};
use aws_smithy_runtime_api::client::http::{SharedHttpClient, SharedHttpConnector};
use aws_smithy_runtime_api::client::orchestrator::HttpRequest;
use common::client as test_client;
use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

const IP1: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

fn shared_client(pool: &ConnectionPool, partition: PartitionId) -> SharedHttpClient {
    SharedHttpClient::new(
        Client::from_partition(pool, partition).expect("declared partition should resolve"),
    )
}

fn connector(client: &SharedHttpClient) -> SharedHttpConnector {
    test_client::connector(client)
}

#[tokio::test]
async fn bounded_waiter_proceeds_after_the_active_h1_returns() {
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
    let pool = ConnectionPool::builder()
        .max_connections_per_host(1)
        .build_http()
        .expect("valid pool");
    let client = SharedHttpClient::new(Client::new(&pool).expect("anonymous partition"));
    let connector = connector(&client);

    let first = test_client::send_request(
        &connector,
        HttpRequest::get(harness.endpoint_url()).expect("valid request"),
    )
    .await
    .expect("first request should reach response headers");
    body_gate
        .wait_until_reached(test_client::WAIT)
        .await
        .expect("first response should reach its body gate");

    let second_connector = connector.clone();
    let second_url = harness.endpoint_url();
    let mut second =
        tokio::spawn(
            async move { test_client::get_and_collect(&second_connector, &second_url).await },
        );
    assert!(
        tokio::time::timeout(Duration::from_millis(100), &mut second)
            .await
            .is_err(),
        "the second request must wait while the only admitted H1 is active"
    );
    assert_eq!(1, harness.tcp_accepted_count());

    body_gate.release();
    let (status, body) = test_client::collect_response(first).await;
    assert_eq!((status, body.as_slice()), (200, b"first-body".as_slice()));
    let (status, body) = second.await.expect("second request task should not panic");
    assert_eq!((status, body.as_slice()), (200, b"second".as_slice()));
    assert_eq!(1, harness.tcp_accepted_count());

    drop(connector);
    drop(client);
    drop(pool);
    harness.shutdown().await.expect("clean harness shutdown");
}

#[tokio::test]
async fn eligible_partition_borrows_the_peer_h1() {
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
    let first_id = PartitionId::from_index(1);
    let second_id = PartitionId::from_index(2);
    let pool = ConnectionPool::builder()
        .partitions([
            Partition::new(first_id, TokioDriverSpawner::current()),
            Partition::new(second_id, TokioDriverSpawner::current()),
        ])
        .connection_reuse_scope(ConnectionReuseScope::Pool)
        .max_connections_per_host(1)
        .build_http()
        .expect("valid pool");
    let first_client = shared_client(&pool, first_id);
    let second_client = shared_client(&pool, second_id);
    let first_connector = connector(&first_client);
    let second_connector = connector(&second_client);

    test_client::get_and_collect(&first_connector, &harness.endpoint_url()).await;
    test_client::get_and_collect(&second_connector, &harness.endpoint_url()).await;

    let request_connections = harness
        .events()
        .into_iter()
        .filter_map(|event| match event {
            ConnectionEvent::Http1Request { connection_id, .. } => Some(connection_id),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(2, request_connections.len());
    assert_eq!(
        request_connections[0], request_connections[1],
        "eligible peer demand should borrow the existing H1"
    );
    assert_eq!(1, harness.tcp_accepted_count());

    drop(first_connector);
    drop(second_connector);
    drop(first_client);
    drop(second_client);
    drop(pool);
    harness.shutdown().await.expect("clean harness shutdown");
}

#[tokio::test]
async fn ineligible_partition_reclaims_peer_capacity() {
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
    let first_id = PartitionId::from_index(1);
    let second_id = PartitionId::from_index(2);
    let pool = ConnectionPool::builder()
        .partitions([
            Partition::new(first_id, TokioDriverSpawner::current()),
            Partition::new(second_id, TokioDriverSpawner::current()),
        ])
        .connection_reuse_scope(ConnectionReuseScope::Partition)
        .max_connections_per_host(1)
        .build_http()
        .expect("valid pool");
    let first_client = shared_client(&pool, first_id);
    let second_client = shared_client(&pool, second_id);
    let first_connector = connector(&first_client);
    let second_connector = connector(&second_client);

    test_client::get_and_collect(&first_connector, &harness.endpoint_url()).await;
    let first_connection = harness
        .events()
        .into_iter()
        .find_map(|event| match event {
            ConnectionEvent::Http1Request { connection_id, .. } => Some(connection_id),
            _ => None,
        })
        .expect("first request should name a connection");
    test_client::get_and_collect(&second_connector, &harness.endpoint_url()).await;

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
        .expect("reclaim should close the ineligible peer H1");
    assert_eq!(2, harness.tcp_accepted_count());

    drop(first_connector);
    drop(second_connector);
    drop(first_client);
    drop(second_client);
    drop(pool);
    harness.shutdown().await.expect("clean harness shutdown");
}

#[tokio::test]
async fn dropping_the_last_pool_handle_closes_idle_connections() {
    let harness = ConnectionTestHarness::builder()
        .endpoint(
            IP1,
            Http1Script::responses([Http1Response::ok().body("response")]),
        )
        .build()
        .await
        .expect("harness should start");
    let pool = ConnectionPool::builder()
        .idle_timeout(None)
        .build_http()
        .expect("valid pool");
    let client = SharedHttpClient::new(Client::new(&pool).expect("anonymous partition"));
    let connector = connector(&client);

    test_client::get_and_collect(&connector, &harness.endpoint_url()).await;
    let connection = harness
        .events()
        .into_iter()
        .find_map(|event| match event {
            ConnectionEvent::Http1Request { connection_id, .. } => Some(connection_id),
            _ => None,
        })
        .expect("request should name a connection");

    drop(connector);
    drop(client);
    drop(pool);
    harness
        .wait_for_event(test_client::WAIT, |event| {
            matches!(
                event,
                ConnectionEvent::ConnectionClosed {
                    connection_id,
                    reason: ConnectionCloseReason::ClientClosed,
                } if *connection_id == connection
            )
        })
        .await
        .expect("pool drop should close its idle connection");
    harness.shutdown().await.expect("clean harness shutdown");
}
