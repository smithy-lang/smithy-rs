/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! Load configuration from AWS Profiles
//!
//! AWS profiles are typically stored in `~/.aws/config` and `~/.aws/credentials`. For more details
//! see the [`load`](parser::load) function.

mod parser;
#[doc(inline)]
pub use parser::{load, Profile, ProfileParseError, ProfileSet, Property};

pub mod app_name;
pub mod credentials;
pub mod region;
pub mod retry_config;
pub mod timeout_config;

#[doc(inline)]
pub use credentials::ProfileFileCredentialsProvider;
#[doc(inline)]
pub use region::ProfileFileRegionProvider;

/// Returns true or false based on whether or not this code is likely running inside an AWS Lambda.
/// [Lambdas set many environment variables](https://docs.aws.amazon.com/lambda/latest/dg/configuration-envvars.html#configuration-envvars-runtime)
/// that we can check.
fn check_is_likely_running_on_a_lambda(environment: &aws_types::os_shim_internal::Env) -> bool {
    // LAMBDA_TASK_ROOT – The path to your Lambda function code.
    environment.get("LAMBDA_TASK_ROOT").is_ok()
}
