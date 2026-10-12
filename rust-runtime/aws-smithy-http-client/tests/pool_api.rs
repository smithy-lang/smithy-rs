/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! External construction APIs for connection-pool integration.

#![cfg(feature = "default-client")]

use aws_smithy_http_client::pool::{self, ConnectionPool, DriverSpawner, Partition, PartitionId};

#[test]
fn callback_partitions_are_available_without_a_test_feature() {
    let id = PartitionId::from_index(4);
    let spawner = DriverSpawner::from_fn(drop);
    let mut builder = ConnectionPool::builder().idle_timeout(None);
    assert!(!builder.has_explicit_partitions());
    builder.set_partitions(Some(Vec::new()));
    assert!(builder.has_explicit_partitions());
    builder.set_partitions(None);
    assert!(!builder.has_explicit_partitions());
    let builder = builder.partitions([Partition::new(id, spawner)]);
    assert!(builder.clone().has_explicit_partitions());
    let pool = builder.build_http().unwrap();
    assert!(pool::Client::from_partition(&pool, id).is_ok());
    assert!(pool::Client::new(&pool).is_err());
}

#[cfg(feature = "rustls-ring")]
#[test]
fn external_constructor_can_store_and_build_the_https_alias() {
    struct Construction {
        transport: pool::HttpsBuilder,
    }

    impl Construction {
        fn build(self) -> Result<ConnectionPool, pool::BuildError> {
            self.transport.build_https()
        }
    }

    let id = PartitionId::from_index(1);
    let transport = ConnectionPool::builder()
        .idle_timeout(None)
        .partitions([Partition::new(id, DriverSpawner::from_fn(drop))])
        .tls_provider(aws_smithy_http_client::tls::Provider::rustls(
            aws_smithy_http_client::tls::rustls_provider::CryptoMode::Ring,
        ));
    assert!(transport.has_explicit_partitions());
    let pool = Construction { transport }.build().unwrap();
    assert!(pool::Client::from_partition(&pool, id).is_ok());
}
