/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::put_test::PutBenchmark;
use crate::{Args, BoxError, BENCH_KEY};
use async_trait::async_trait;
use aws_config::SdkConfig;
use aws_sdk_s3 as s3;
use aws_sdk_s3::Client;
use aws_smithy_http::byte_stream::ByteStream;
use s3::types::CompletedMultipartUpload;
use s3::types::CompletedPart;
use std::io::SeekFrom;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::time::timeout;

pub(crate) struct PutObjectMultipart;

#[async_trait]
impl PutBenchmark for PutObjectMultipart {
    type Setup = Vec<Client>;

    async fn prepare(&self, conf: &SdkConfig) -> Self::Setup {
        let clients = (0..32).map(|_| Client::new(&conf)).collect::<Vec<_>>();
        for client in &clients {
            let _ = client.list_buckets().send().await;
        }
        clients
    }

    async fn do_put(
        &self,
        state: Self::Setup,
        target_key: &str,
        local_file: &Path,
        args: &Args,
    ) -> Result<(), BoxError> {
        put_object_multipart(&state, args, target_key, local_file).await
    }
}

pub async fn put_object_multipart(
    client: &[s3::Client],
    args: &Args,
    target_key: &str,
    path: &Path,
) -> Result<(), BoxError> {
    let upload_id = client[0]
        .create_multipart_upload()
        .bucket(&args.bucket)
        .key(target_key)
        .send()
        .await?
        .upload_id
        .expect("missing upload id");

    let mut part_count = args.size_bytes / args.part_size_bytes + 1;
    let mut size_of_last_part = args.size_bytes % args.part_size_bytes;
    if size_of_last_part == 0 {
        size_of_last_part = args.part_size_bytes;
        part_count -= 1;
    }

    let semaphore = Arc::new(Semaphore::new(args.concurrency));
    let mut tasks = Vec::new();
    for part in 0..part_count {
        let offset = args.part_size_bytes * part;
        let length = if part == part_count - 1 {
            size_of_last_part
        } else {
            args.part_size_bytes
        };
        let permit = semaphore.clone().acquire_owned().await?;
        tasks.push(tokio::spawn(upload_part_retry_on_timeout(
            permit,
            client[part as usize % client.len()].clone(),
            args.bucket.clone(),
            upload_id.clone(),
            path.to_path_buf(),
            offset,
            length,
            part,
            Duration::from_millis(args.part_upload_timeout_millis),
        )));
    }
    let mut parts = Vec::new();
    for task in tasks {
        parts.push(task.await??);
    }

    client[0]
        .complete_multipart_upload()
        .bucket(&args.bucket)
        .key(BENCH_KEY)
        .upload_id(&upload_id)
        .multipart_upload(
            CompletedMultipartUpload::builder()
                .set_parts(Some(parts))
                .build(),
        )
        .send()
        .await?;

    Ok(())
}

async fn upload_part_retry_on_timeout(
    permit: OwnedSemaphorePermit,
    client: s3::Client,
    bucket: String,
    upload_id: String,
    path: PathBuf,
    offset: u64,
    length: u64,
    part: u64,
    timeout_dur: Duration,
) -> Result<CompletedPart, BoxError> {
    loop {
        match timeout(
            timeout_dur,
            upload_part(&client, &bucket, &upload_id, &path, offset, length, part),
        )
        .await
        {
            Ok(res) => {
                drop(permit);
                return res;
            }
            Err(_) => tracing::warn!(id = ?part, "timeout!"),
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn upload_part(
    client: &s3::Client,
    bucket: &str,
    upload_id: &str,
    path: &Path,
    offset: u64,
    length: u64,
    part: u64,
) -> Result<CompletedPart, BoxError> {
    let start = SystemTime::now();
    let mut file = File::open(path).await?;
    file.seek(SeekFrom::Start(offset)).await?;
    let mut buf = vec![0; length as usize];
    file.read_exact(&mut buf).await?;
    let stream = ByteStream::from(buf);
    let part_output = client
        .upload_part()
        .key(BENCH_KEY)
        .bucket(bucket)
        .upload_id(upload_id)
        .body(stream)
        .part_number(part as i32 + 1) // S3 takes a 1-based index
        .send()
        .await?;
    tracing::debug!(part = ?part, upload_duration = ?start.elapsed().unwrap(), "upload-part");
    Ok(CompletedPart::builder()
        .part_number(part as i32 + 1)
        .e_tag(part_output.e_tag.expect("must have an e-tag"))
        .build())
}
