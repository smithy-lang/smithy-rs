/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::{Args, BoxError, BENCH_KEY};
use aws_sdk_s3 as s3;
use std::fmt;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::watch::channel;
use tokio::sync::Semaphore;

pub async fn get_object_multipart(
    client: &s3::Client,
    args: &Args,
    path: &Path,
) -> Result<(), BoxError> {
    let mut part_count = (args.size_bytes / args.part_size_bytes + 1) as i64;
    let mut size_of_last_part = (args.size_bytes % args.part_size_bytes) as i64;
    if size_of_last_part == 0 {
        size_of_last_part = args.part_size_bytes as i64;
        part_count -= 1;
    }

    let mut ranges = (0..part_count).map(|i| {
        if i == part_count - 1 {
            let start = i * args.part_size_bytes as i64;
            ContentRange::new(start, start + size_of_last_part - 1)
        } else {
            ContentRange::new(
                i * args.part_size_bytes as i64,
                (i + 1) * args.part_size_bytes as i64 - 1,
            )
        }
    });
    let (tx, rx) = channel(ranges.next().unwrap());
    for range in ranges {
        tx.send(range)?;
    }

    let semaphore = Arc::new(Semaphore::new(args.concurrency));
    let mut tasks = Vec::new();
    for _ in 0..part_count {
        let semaphore = semaphore.clone();
        let client = client.clone();
        let bucket = args.bucket.clone();
        let mut rx = rx.clone();
        tasks.push(tokio::spawn(async move {
            let _permit = semaphore.acquire().await?;
            let range = rx.borrow_and_update().to_string();

            let part = client
                .get_object()
                .bucket(bucket)
                .key(BENCH_KEY)
                .range(range)
                .send()
                .await?;

            Result::<_, BoxError>::Ok(part.body)
        }));
    }
    for task in tasks {
        let mut body = task.await??.into_async_read();
        let mut file = tokio::fs::File::create(path).await?;
        tokio::io::copy(&mut body, &mut file).await?;
    }

    Ok(())
}

#[derive(Debug)]
struct ContentRange {
    start: i64,
    end: i64,
}

impl ContentRange {
    fn new(start: i64, end: i64) -> Self {
        Self { start, end }
    }
}

impl fmt::Display for ContentRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "bytes={}-{}", self.start, self.end)
    }
}
