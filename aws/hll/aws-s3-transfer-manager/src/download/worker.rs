/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
use crate::download::context::DownloadContext;
use crate::download::header;
use crate::error;
use crate::error::TransferError;
use aws_sdk_s3::operation::get_object::builders::GetObjectInputBuilder;
use aws_smithy_types::body::SdkBody;
use aws_smithy_types::byte_stream::{AggregatedBytes, ByteStream};
use std::ops::RangeInclusive;
use std::{cmp, mem};
use tokio::sync::mpsc;
use tracing::Instrument;

// FIXME - should probably be enum ChunkRequest { Range(..), Part(..) } or have an inner field like such
#[derive(Debug, Clone)]
pub(super) struct ChunkRequest {
    // byte range to download
    pub(super) range: RangeInclusive<u64>,
    pub(super) input: GetObjectInputBuilder,
    // sequence number
    pub(super) seq: u64,
}

impl ChunkRequest {
    /// Size of this chunk request in bytes
    pub(super) fn size(&self) -> u64 {
        self.range.end() - self.range.start() + 1
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ChunkResponse {
    // TODO(aws-sdk-rust#1159, design) - consider PartialOrd for ChunkResponse and hiding `seq` as internal only detail
    // the seq number
    pub(crate) seq: u64,
    // chunk data
    pub(crate) data: Option<AggregatedBytes>,
}

/// Worker function that processes requests from the `requests` channel and
/// sends the result back on the `completed` channel.
pub(super) async fn download_chunks(
    ctx: DownloadContext,
    requests: async_channel::Receiver<ChunkRequest>,
    completed: mpsc::Sender<Result<ChunkResponse, TransferError>>,
) {
    while let Ok(request) = requests.recv().await {
        let seq = request.seq;
        tracing::trace!("worker recv'd request for chunk seq {seq}");

        let result = download_chunk(&ctx, request)
            .instrument(tracing::debug_span!("download-chunk", seq = seq))
            .await;

        if let Err(err) = completed.send(result).await {
            tracing::debug!(error = ?err, "chunk worker send failed");
            return;
        }
    }

    tracing::trace!("req channel closed, worker finished");
}

/// Download an individual chunk of data (range / part)
async fn download_chunk(
    ctx: &DownloadContext,
    request: ChunkRequest,
) -> Result<ChunkResponse, TransferError> {
    let mut resp = request
        .input
        .send_with(&ctx.client)
        .await
        .map_err(error::chunk_failed)?;

    let body = mem::replace(&mut resp.body, ByteStream::new(SdkBody::taken()));

    let bytes = body
        .collect()
        .instrument(tracing::debug_span!("collect-body", seq = request.seq))
        .await
        .map_err(error::chunk_failed)?;

    Ok(ChunkResponse {
        seq: request.seq,
        data: Some(bytes),
    })
}

pub(super) async fn distribute_work(
    remaining: RangeInclusive<u64>,
    input: GetObjectInputBuilder,
    part_size: u64,
    start_seq: u64,
    tx: async_channel::Sender<ChunkRequest>,
) {
    let end = *remaining.end();
    let mut pos = *remaining.start();
    let mut remaining = end - pos + 1;
    let mut seq = start_seq;

    while remaining > 0 {
        let start = pos;
        let end_inclusive = cmp::min(pos + part_size - 1, end);

        let chunk_req = next_chunk(start, end_inclusive, seq, input.clone());
        tracing::trace!(
            "distributing chunk(size={}): {:?}",
            chunk_req.size(),
            chunk_req
        );
        let chunk_size = chunk_req.size();
        tx.send(chunk_req).await.expect("channel open");

        seq += 1;
        remaining -= chunk_size;
        tracing::trace!("remaining = {}", remaining);
        pos += chunk_size;
    }

    tracing::trace!("work fully distributed");
    tx.close();
}

fn next_chunk(
    start: u64,
    end_inclusive: u64,
    seq: u64,
    input: GetObjectInputBuilder,
) -> ChunkRequest {
    let range = start..=end_inclusive;
    let input = input.range(header::Range::bytes_inclusive(start, end_inclusive));
    ChunkRequest { seq, range, input }
}

#[cfg(test)]
mod tests {
    use crate::download::header;
    use crate::download::worker::distribute_work;
    use aws_sdk_s3::operation::get_object::builders::GetObjectInputBuilder;
    use std::ops::RangeInclusive;

    #[tokio::test]
    async fn test_distribute_work() {
        let rem = 0..=90u64;
        let part_size = 20;
        let input = GetObjectInputBuilder::default();
        let (tx, rx) = async_channel::unbounded();

        tokio::spawn(distribute_work(rem, input, part_size, 0, tx));

        let mut chunks = Vec::new();
        while let Ok(chunk) = rx.recv().await {
            chunks.push(chunk);
        }

        let expected_ranges = vec![0..=19u64, 20..=39u64, 40..=59u64, 60..=79u64, 80..=90u64];

        let actual_ranges: Vec<RangeInclusive<u64>> =
            chunks.iter().map(|c| c.range.clone()).collect();

        assert_eq!(expected_ranges, actual_ranges);
        assert!(rx.is_closed());

        for (i, chunk) in chunks.iter().enumerate() {
            assert_eq!(i as u64, chunk.seq);
            let expected_range_header =
                header::Range::bytes_inclusive(*chunk.range.start(), *chunk.range.end())
                    .to_string();

            assert_eq!(
                expected_range_header,
                chunk.input.get_range().clone().expect("range header set")
            );
        }
    }
}
