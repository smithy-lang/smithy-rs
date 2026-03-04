/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::fmt;
use std::future::Future;
use std::pin::Pin;

use aws_config::SdkConfig;
use tracing::{info_span, Instrument};

use crate::current_canary::{ec2_canary, s3_canary, sts_canary};

#[macro_export]
macro_rules! mk_canary {
    ($name: expr, $run_canary: expr) => {
        pub(crate) fn mk_canary(
            sdk_config: &aws_config::SdkConfig,
            env: &CanaryEnv,
        ) -> Option<(&'static str, $crate::canary::CanaryFuture)> {
            #[allow(clippy::redundant_closure_call)]
            Some(($name, Box::pin($run_canary(sdk_config, env))))
        }
    };
}

pub fn get_canaries_to_run(
    sdk_config: SdkConfig,
    env: CanaryEnv,
) -> Vec<(&'static str, CanaryFuture)> {
    let canaries = vec![
        sts_canary::mk_canary(&sdk_config, &env),
        s3_canary::mk_canary(&sdk_config, &env),
        ec2_canary::mk_canary(&sdk_config, &env),
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

#[derive(Clone)]
pub struct CanaryEnv {
    pub(crate) s3_bucket_name: String,
}

impl fmt::Debug for CanaryEnv {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CanaryEnv")
            .field("s3_bucket_name", &self.s3_bucket_name)
            .finish()
    }
}

impl CanaryEnv {
    pub fn from_env() -> Self {
        let s3_bucket_name =
            std::env::var("CANARY_S3_BUCKET_NAME").expect("CANARY_S3_BUCKET_NAME must be set");
        Self { s3_bucket_name }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct CanaryError(pub String);

pub type CanaryFuture = Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>>;
