/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use super::phase::Phase;
use aws_smithy_http::result::SdkError;
use aws_smithy_runtime_api::client::interceptors::context::Error;
use aws_smithy_runtime_api::client::orchestrator::HttpResponse;
use aws_smithy_runtime_api::config_bag::ConfigBag;

pub(super) async fn orchestrate_auth(
    _dispatch_phase: Phase,
    _cfg: &ConfigBag,
) -> Result<Phase, SdkError<Error, HttpResponse>> {
    todo!()
}
