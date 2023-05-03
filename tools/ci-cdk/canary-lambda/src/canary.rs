/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::env;
use std::fmt;
use std::future::Future;
use std::pin::Pin;

use aws_config::SdkConfig;
use tracing::{info_span, Instrument};

use crate::current_canary::paginator_canary;
use crate::current_canary::{s3_canary, transcribe_canary};

#[macro_export]
macro_rules! mk_canary {
    ($name: expr, $run_canary: expr) => {
        pub(crate) fn mk_canary(
            sdk_config: &aws_config::SdkConfig,
            env: &CanaryEnv,
        ) -> Option<(&'static str, $crate::canary::CanaryFuture)> {
            Some(($name, Box::pin($run_canary(sdk_config, env))))
        }
    };
}

pub fn get_canaries_to_run(
    sdk_config: SdkConfig,
    env: CanaryEnv,
) -> Vec<(&'static str, CanaryFuture)> {
    let canaries = vec![
        paginator_canary::mk_canary(&sdk_config, &env),
        s3_canary::mk_canary(&sdk_config, &env),
        transcribe_canary::mk_canary(&sdk_config, &env),
    ];

    canaries
        .into_iter()
        .flatten()
        .map(|(name, fut)| {
            (
                name,
                Box::pin(fut.instrument(info_span!("run_canary", name = name))) as _,
            )
        })
        .collect()
}

pub struct CanaryEnv {
    pub(crate) s3_bucket_name: String,
    pub(crate) expected_transcribe_result: String,
    #[allow(dead_code)]
    pub(crate) page_size: usize,
}

impl fmt::Debug for CanaryEnv {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CanaryEnv")
            .field("s3_bucket_name", &"*** redacted ***")
            .field(
                "expected_transcribe_result",
                &self.expected_transcribe_result,
            )
            .finish()
    }
}

impl CanaryEnv {
    pub fn from_env() -> Self {
        // S3 bucket name to test against
        let s3_bucket_name =
            env::var("CANARY_S3_BUCKET_NAME").expect("CANARY_S3_BUCKET_NAME must be set");

        // Expected transcription from Amazon Transcribe from the embedded audio file.
        // This is an environment variable so that the code doesn't need to be changed if
        // Amazon Transcribe starts returning different output for the same audio.
        let expected_transcribe_result = env::var("CANARY_EXPECTED_TRANSCRIBE_RESULT")
            .unwrap_or_else(|_| {
                "Good day to you transcribe. This is Polly talking to you from the Rust ST K."
                    .to_string()
            });

        let page_size = env::var("PAGE_SIZE")
            .map(|ps| ps.parse::<usize>())
            .unwrap_or_else(|_| Ok(16))
            .expect("invalid page size");

        Self {
            s3_bucket_name,
            expected_transcribe_result,
            page_size,
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct CanaryError(pub String);

pub type CanaryFuture = Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>>;
