/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::{Args, BoxError, BENCH_KEY};
use async_trait::async_trait;
use aws_config::SdkConfig;
use aws_sdk_s3 as s3;
use aws_sdk_s3::Client;
use aws_smithy_http::byte_stream::AggregatedBytes;
use std::fmt;
use std::fs::File;
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use crate::get_test::GetBenchmark;
use tokio::sync::Semaphore;
use tokio::task::spawn_blocking;
use tokio::time::timeout;
use tracing::{info_span, Instrument};

pub(crate) struct GetObjectMultipart {}
impl GetObjectMultipart {
    pub(crate) fn new() -> Self {
        Self {}
    }
}

#[async_trait]
impl GetBenchmark for GetObjectMultipart {
    type Setup = Vec<Client>;

    async fn prepare(&self, conf: &SdkConfig) -> Self::Setup {
        let clients = (0..32).map(|_| Client::new(&conf)).collect::<Vec<_>>();
        for client in &clients {
            let _ = client.list_buckets().send().await;
        }
        clients
    }

    async fn do_get(
        &self,
        state: Self::Setup,
        target_path: &Path,
        args: &Args,
    ) -> Result<PathBuf, BoxError> {
        get_object_multipart(&state, args, target_path, &args.bucket, BENCH_KEY).await?;
        Ok(target_path.to_path_buf())
    }
}

pub async fn get_object_multipart(
    clients: &[s3::Client],
    args: &Args,
    target_path: &Path,
    bucket: &str,
    key: &str,
) -> Result<(), BoxError> {
    let mut part_count = (args.size_bytes / args.part_size_bytes + 1) as u64;
    let mut size_of_last_part = (args.size_bytes % args.part_size_bytes) as u64;
    if size_of_last_part == 0 {
        size_of_last_part = args.part_size_bytes as u64;
        part_count -= 1;
    }

    let ranges = (0..part_count).map(|i| {
        if i == part_count - 1 {
            let start = i * args.part_size_bytes as u64;
            ContentRange::new(start, start + size_of_last_part - 1)
        } else {
            ContentRange::new(
                i * args.part_size_bytes as u64,
                (i + 1) * args.part_size_bytes as u64 - 1,
            )
        }
    });

    let semaphore = Arc::new(Semaphore::new(args.concurrency));
    let mut tasks = Vec::new();
    let file = Arc::new(File::create(target_path)?);
    for (id, range) in ranges.enumerate() {
        let semaphore = semaphore.clone();
        let client = clients[id % clients.len()].clone();
        let file = file.clone();
        let bucket = bucket.to_string();
        let key = key.to_string();
        tasks.push(tokio::spawn(
            async move {
                let _permit = semaphore.acquire_owned().await?;

                let start = SystemTime::now();
                tracing::debug!(range = ?range);

                let body =
                    download_part_retry_on_timeout(id, &range, &client, &bucket, &key).await?;
                tracing::debug!(id =? id, load_duration = ?start.elapsed().unwrap());
                let mut offset = range.start;
                let write_duration = SystemTime::now();
                spawn_blocking(move || {
                    for part in body.into_segments() {
                        file.write_all_at(&part, offset)?;
                        offset += part.len() as u64;
                    }
                    Ok::<_, BoxError>(())
                })
                .await??;
                tracing::debug!(id =? id, write_duration = ?write_duration.elapsed().unwrap());
                Result::<_, BoxError>::Ok(())
            }
            .instrument(info_span!("run-collect-part", id = id)),
        ));
    }
    for task in tasks {
        task.await??;
    }

    Ok(())
}

async fn download_part_retry_on_timeout(
    id: usize,
    range: &ContentRange,
    client: &Client,
    bucket: &str,
    key: &str,
) -> Result<AggregatedBytes, BoxError> {
    loop {
        match timeout(
            Duration::from_millis(1000),
            download_part(id, range, client, bucket, key),
        )
        .await
        {
            Ok(result) => return result,
            Err(_) => tracing::warn!("get part timeout"),
        }
    }
}

async fn download_part(
    id: usize,
    range: &ContentRange,
    client: &Client,
    bucket: &str,
    key: &str,
) -> Result<AggregatedBytes, BoxError> {
    let part = client
        .get_object()
        .bucket(bucket)
        .key(key)
        .range(range.to_string())
        .send()
        .instrument(info_span!("get_object", id = id))
        .await?;

    let body = part
        .body
        .collect()
        .instrument(info_span!("collect-body", id = id))
        .await?;
    Ok(body)
}

#[derive(Debug)]
struct ContentRange {
    start: u64,
    end: u64,
}

impl ContentRange {
    fn new(start: u64, end: u64) -> Self {
        Self { start, end }
    }
}

impl fmt::Display for ContentRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "bytes={}-{}", self.start, self.end)
    }
}
