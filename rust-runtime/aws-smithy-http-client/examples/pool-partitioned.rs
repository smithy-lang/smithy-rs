/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Splitting a pool into partitions.
//!
//! A partition is a group of connections that share a driver runtime and an
//! optional network interface. A connection's driver task is pinned to the
//! runtime that created it, so partitioning keeps each connection's I/O on
//! its owning runtime while one shared pool enforces a single global
//! connection budget.
//!
//! This example declares two interface-bound partitions, builds a `Client`
//! for one, and reads per-partition connection state for an authority.
//!
//! Binding a partition to a network interface ([`Partition::interface`]) is
//! only available on Android, Fuchsia, and Linux, so this example builds its
//! interface-bound topology only on those targets.

#[cfg(any(target_os = "android", target_os = "fuchsia", target_os = "linux"))]
use aws_smithy_http_client::pool::{
    Authority, Client, CrossPartitionPolicy, Partition, PartitionId, SharedPool, TokioDriverSpawner,
};
#[cfg(any(target_os = "android", target_os = "fuchsia", target_os = "linux"))]
use aws_smithy_http_client::tls::{self, rustls_provider::CryptoMode};

#[cfg(any(target_os = "android", target_os = "fuchsia", target_os = "linux"))]
#[tokio::main]
async fn main() {
    // Declare one partition per interface. `PartitionId` is a caller-owned
    // label; the driver spawner sets the runtime a partition's connection
    // drivers run on (here, the current runtime).
    let pool = SharedPool::builder()
        .tls_provider(tls::Provider::Rustls(CryptoMode::AwsLc))
        // A single ceiling shared across all partitions.
        .max_connections(2_000)
        // `Never` (the default) keeps each partition's requests on its own
        // connections. `PreferLocal` lets a cap-bound partition borrow a
        // same-interface peer's idle connection instead of waiting.
        .cross_partition_policy(CrossPartitionPolicy::Never)
        .partitions([
            Partition::new(PartitionId::from_index(0), TokioDriverSpawner::current())
                .interface("eth0"),
            Partition::new(PartitionId::from_index(1), TokioDriverSpawner::current())
                .interface("eth1"),
        ])
        .build_https();

    // `Client::new` targets the default partition; `from_partition` targets a
    // declared one. The client is a cheap handle and implements `HttpClient`.
    let _client = Client::from_partition(&pool, PartitionId::from_index(0));

    // `stats` reports per-partition connection counts for an authority. Only
    // partitions that have opened a connection to it appear. The counts are a
    // lock-free, point-in-time read.
    let authority = Authority::from_host("example.com");
    for (partition, s) in pool.stats(&authority).iter() {
        // established: connections that exist; idle(): those not in use;
        // establishing: handshakes in flight.
        println!(
            "partition {:?}: established={} idle={} establishing={}",
            partition,
            s.established,
            s.idle(),
            s.establishing,
        );
    }
}

/// Interface binding is unavailable on this target; see the module docs.
#[cfg(not(any(target_os = "android", target_os = "fuchsia", target_os = "linux")))]
fn main() {
    eprintln!(
        "pool-partitioned: interface-bound partitions require Android, Fuchsia, \
         or Linux; nothing to demonstrate on this target"
    );
}
