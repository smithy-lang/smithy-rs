/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

pub mod body;
mod context;
mod discovery;
mod handle;
mod header;
mod object_meta;
mod worker;

use crate::download::body::Body;
use crate::download::discovery::{discover_obj, ObjectDiscovery};
use crate::download::handle::DownloadHandle;
use crate::download::worker::{distribute_work, download_chunks, ChunkResponse};
use crate::error::TransferError;
use crate::{MEBIBYTE, MIN_PART_SIZE};
use aws_sdk_s3::operation::get_object::builders::{GetObjectFluentBuilder, GetObjectInputBuilder};
use aws_types::SdkConfig;
use context::DownloadContext;
use std::{cmp, mem};
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tracing::Instrument;

// TODO(aws-sdk-rust#1159) - need to set User-Agent header value for SEP, e.g. `ft/hll#s3-transfer`

/// Request type for downloading a single object
#[derive(Debug)]
#[non_exhaustive]
pub struct DownloadRequest {
    pub(crate) input: GetObjectInputBuilder,
}

// FIXME - should probably be TryFrom since checksums may conflict?
impl From<GetObjectFluentBuilder> for DownloadRequest {
    fn from(value: GetObjectFluentBuilder) -> Self {
        Self {
            input: value.as_input().clone(),
        }
    }
}

impl From<GetObjectInputBuilder> for DownloadRequest {
    fn from(value: GetObjectInputBuilder) -> Self {
        Self { input: value }
    }
}

/// Fluent style builder for [Downloader]
#[derive(Debug, Clone)]
pub struct Builder {
    target_part_size_bytes: u64,
    checksum_validation_enabled: bool,
    // TODO(design): should we instead consider an enum here allows for not only explicit but also
    // an "Auto" mode that allows us to control the concurrency actually used based on overall transfer and part size?
    concurrency: usize,
    sdk_config: Option<SdkConfig>,
}

impl Builder {
    fn new() -> Self {
        Self {
            target_part_size_bytes: 8 * MEBIBYTE,
            checksum_validation_enabled: true,
            concurrency: 8,
            sdk_config: None,
        }
    }

    /// Size of parts the object will be downloaded in, in bytes.
    ///
    /// The minimum part size is 5 MiB and any value given less than that will be rounded up.
    /// Defaults is 8 MiB.
    pub fn target_part_size(mut self, size_bytes: u64) -> Self {
        self.target_part_size_bytes = cmp::min(size_bytes, MIN_PART_SIZE);
        self
    }

    /// Set whether checksum validation is enabled.
    ///
    /// Default is true.
    pub fn enable_checksum_validation(mut self, enabled: bool) -> Self {
        self.checksum_validation_enabled = enabled;
        self
    }

    /// Set the configuration used by the S3 client
    pub fn sdk_config(mut self, config: SdkConfig) -> Self {
        self.sdk_config = Some(config);
        self
    }

    /// Set the concurrency level this component is allowed to use.
    ///
    /// This sets the maximum number of concurrent in-flight requests.
    /// Default is 8.
    pub fn concurrency(mut self, concurrency: usize) -> Self {
        self.concurrency = concurrency;
        self
    }

    pub fn build(self) -> Downloader {
        self.into()
    }
}

impl From<Builder> for Downloader {
    fn from(value: Builder) -> Self {
        let sdk_config = value
            .sdk_config
            .unwrap_or_else(|| SdkConfig::builder().build());
        let client = aws_sdk_s3::Client::new(&sdk_config);
        Self {
            target_part_size_bytes: value.target_part_size_bytes,
            checksum_validation_enabled: value.checksum_validation_enabled,
            concurrency: value.concurrency,
            client,
        }
    }
}

/// Download an object in the most efficient way possible by splitting the request into
/// concurrent requests (e.g. using ranged GET or part number).
#[derive(Debug, Clone)]
pub struct Downloader {
    target_part_size_bytes: u64,
    checksum_validation_enabled: bool,
    concurrency: usize,
    client: aws_sdk_s3::client::Client,
}

impl Downloader {
    /// Create a new [Builder]
    pub fn builder() -> Builder {
        Builder::new()
    }

    /// Download a single object from S3.
    ///
    /// A single logical request may be split into many concurrent ranged `GetObject` requests
    /// to improve throughput.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::error::Error;
    /// use aws_sdk_s3::operation::get_object::builders::GetObjectInputBuilder;
    /// use aws_s3_transfer_manager::download::{Downloader, DownloadRequest};
    ///
    /// async fn get_object(client: Downloader) -> Result<(), Box<dyn Error>> {
    ///     let request = GetObjectInputBuilder::default()
    ///         .bucket("my-bucket")
    ///         .key("my-key")
    ///         .into();
    ///
    ///     let handle = client.download(request).await?;
    ///     // process data off handle...
    ///     Ok(())
    /// }
    /// ```
    pub async fn download(&self, req: DownloadRequest) -> Result<DownloadHandle, TransferError> {
        // if there is a part number then just send the default request
        if req.input.get_part_number().is_some() {
            todo!("single part download not implemented")
        }

        let ctx = DownloadContext {
            client: self.client.clone(),
            target_part_size: self.target_part_size_bytes,
        };

        // make initial discovery about the object size, metadata, possibly first chunk
        let mut discovery = discover_obj(&ctx, &req).await?;
        let (comp_tx, comp_rx) = mpsc::channel(self.concurrency);
        let start_seq = handle_discovery_chunk(&mut discovery, &comp_tx).await;

        // spawn all work into the same JoinSet such that when the set is dropped all tasks are cancelled.
        let mut tasks = JoinSet::new();

        if !discovery.remaining.is_empty() {
            // start assigning work
            let (work_tx, work_rx) = async_channel::bounded(self.concurrency);
            let input = req.input.clone();
            let part_size = self.target_part_size_bytes;
            let rem = discovery.remaining.clone();

            tasks.spawn(distribute_work(rem, input, part_size, start_seq, work_tx));

            for i in 0..self.concurrency {
                let worker = download_chunks(ctx.clone(), work_rx.clone(), comp_tx.clone())
                    .instrument(tracing::debug_span!("chunk-downloader", worker = i));
                tasks.spawn(worker);
            }
        }

        // Drop our half of the completion channel. When all workers drop theirs, the channel is closed.
        drop(comp_tx);

        let handle = DownloadHandle {
            // FIXME(aws-sdk-rust#1159) - initial object discovery for a range/first-part will not
            //   have the correct metadata w.r.t. content-length and maybe others for the whole object.
            object_meta: discovery.meta,
            body: Body::new(comp_rx),
            tasks,
        };

        Ok(handle)
    }
}

/// Handle possibly sending the first chunk of data received through discovery. Returns
/// the starting sequence number to use for remaining chunks.
async fn handle_discovery_chunk(
    discovery: &mut ObjectDiscovery,
    completed: &mpsc::Sender<Result<ChunkResponse, TransferError>>,
) -> u64 {
    let mut start_seq = 0;
    if let Some(initial_data) = mem::replace(&mut discovery.initial_chunk, None) {
        let chunk = ChunkResponse {
            seq: start_seq,
            data: Some(initial_data),
            object_meta: Some(discovery.meta.clone()),
        };
        completed.send(Ok(chunk)).await.expect("initial chunk");
        start_seq = 1;
    }
    start_seq
}
