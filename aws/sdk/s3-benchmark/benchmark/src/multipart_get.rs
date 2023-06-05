/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::{Args, BoxError, BENCH_KEY};
use aws_sdk_s3 as s3;
use aws_smithy_http::byte_stream::ByteStream;
use std::fmt;
use std::path::Path;
use std::sync::Arc;
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

    let ranges = (0..part_count).map(|i| {
        if i == part_count - 1 {
            let start = i * args.part_size_bytes as i64;
            ContentRange::new(start, start + size_of_last_part - 1, size_of_last_part)
        } else {
            ContentRange::new(
                i * args.part_size_bytes as i64,
                (i + 1) * args.part_size_bytes as i64 - 1,
                args.part_size_bytes as i64,
            )
        }
    });

    let semaphore = Arc::new(Semaphore::new(args.concurrency));
    let mut tasks = Vec::new();
    for range in ranges {
        tasks.push(tokio::spawn(download_part(
            semaphore.clone(),
            client.clone(),
            args.bucket.clone(),
            range,
        )));
    }
    let mut file = tokio::fs::File::create(path).await?;
    for task in tasks {
        let mut body = task.await??.into_async_read();
        tokio::io::copy(&mut body, &mut file).await?;
    }

    Ok(())
}

struct ContentRange {
    start: i64,
    end: i64,
    length: i64,
}

impl ContentRange {
    fn new(start: i64, end: i64, length: i64) -> Self {
        Self { start, end, length }
    }
}

impl fmt::Display for ContentRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "bytes={}-{}/{}", self.start, self.end, self.length)
    }
}

async fn download_part(
    semaphore: Arc<Semaphore>,
    client: s3::Client,
    bucket: String,
    range: ContentRange,
) -> Result<ByteStream, BoxError> {
    let _permit = semaphore.acquire().await?;

    let part = client
        .get_object()
        .bucket(bucket)
        .key(BENCH_KEY)
        .range(range.to_string())
        .send()
        .await?;
    Ok(part.body)
}
