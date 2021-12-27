/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use crate::{s3_canary, transcribe_canary};
use aws_sdk_s3 as s3;
use aws_sdk_transcribestreaming as transcribe;
use std::env;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use tracing::{info_span, Instrument};

pub fn get_canaries_to_run(clients: Clients, env: CanaryEnv) -> Vec<(&'static str, CanaryFuture)> {
    vec![
        (
            "s3_canary",
            Box::pin(
                s3_canary::s3_canary(clients.s3, env.s3_bucket_name)
                    .instrument(info_span!("s3_canary")),
            ),
        ),
        (
            "transcribe_canary",
            Box::pin(
                transcribe_canary::transcribe_canary(
                    clients.transcribe,
                    env.expected_transcribe_result,
                )
                .instrument(info_span!("transcribe_canary")),
            ),
        ),
    ]
}

#[derive(Clone)]
pub struct Clients {
    pub s3: s3::Client,
    pub transcribe: transcribe::Client,
}

impl Clients {
    pub async fn initialize() -> Self {
        let config = aws_config::load_from_env().await;
        Self {
            s3: s3::Client::new(&config),
            transcribe: transcribe::Client::new(&config),
        }
    }
}

pub struct CanaryEnv {
    s3_bucket_name: String,
    expected_transcribe_result: String,
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
            .expect("CANARY_EXPECTED_TRANSCRIBE_RESULT must be set");

        Self {
            s3_bucket_name,
            expected_transcribe_result,
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct CanaryError(pub String);

pub type CanaryFuture = Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>>;
