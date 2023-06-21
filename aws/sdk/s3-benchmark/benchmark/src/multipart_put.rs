/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::{Args, BoxError, BENCH_KEY};
use aws_sdk_s3 as s3;
use aws_smithy_http::byte_stream::ByteStream;
use aws_smithy_http::byte_stream::Length;
use s3::types::CompletedMultipartUpload;
use s3::types::CompletedPart;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Semaphore;

pub async fn put_object_multipart(
    client: &s3::Client,
    args: &Args,
    path: &Path,
) -> Result<(), BoxError> {
    let upload_id = client
        .create_multipart_upload()
        .bucket(&args.bucket)
        .key(BENCH_KEY)
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
        tasks.push(tokio::spawn(upload_part(
            semaphore.clone(),
            client.clone(),
            args.bucket.clone(),
            upload_id.clone(),
            path.to_path_buf(),
            offset,
            length,
            part,
        )));
    }
    let mut parts = Vec::new();
    for task in tasks {
        parts.push(task.await??);
    }

    client
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

#[allow(clippy::too_many_arguments)]
async fn upload_part(
    semaphore: Arc<Semaphore>,
    client: s3::Client,
    bucket: String,
    upload_id: String,
    path: PathBuf,
    offset: u64,
    length: u64,
    part: u64,
) -> Result<CompletedPart, BoxError> {
    let _permit = semaphore.acquire().await?;

    let stream = ByteStream::read_from()
        .path(path)
        .offset(offset)
        .length(Length::Exact(length))
        .build()
        .await?;
    let part_output = client
        .upload_part()
        .key(BENCH_KEY)
        .bucket(bucket)
        .upload_id(upload_id)
        .body(stream)
        .part_number(part as i32 + 1) // S3 takes a 1-based index
        .send()
        .await?;
    Ok(CompletedPart::builder()
        .part_number(part as i32 + 1)
        .e_tag(part_output.e_tag.expect("must have an e-tag"))
        .build())
}
