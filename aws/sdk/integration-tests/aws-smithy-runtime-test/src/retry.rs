/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_runtime::{BoxError, HttpRequest, HttpResponse, RetryStrategy};
use aws_smithy_runtime_api::config_bag::ConfigBag;
use aws_smithy_runtime_api::interceptors::InterceptorContext;
use aws_smithy_runtime_api::runtime_plugin::RuntimePlugin;

#[derive(Debug)]
pub struct GetObjectRetryStrategy {}

impl GetObjectRetryStrategy {
    pub fn new() -> Self {
        Self {}
    }
}

impl RuntimePlugin for GetObjectRetryStrategy {
    fn configure(&self, _cfg: &mut ConfigBag) -> Result<(), BoxError> {
        // TODO(orchestrator) put a retry strategy in the bag
        Ok(())
    }
}

impl RetryStrategy for GetObjectRetryStrategy {
    fn should_attempt_initial_request(&self, _cfg: &ConfigBag) -> Result<(), BoxError> {
        todo!()
    }

    fn should_attempt_retry(
        &self,
        _context: &InterceptorContext<HttpRequest, HttpResponse>,
        _cfg: &ConfigBag,
    ) -> Result<bool, BoxError> {
        todo!()
    }
}

//     retry_classifier: Arc::new(
//         |res: Result<&SdkSuccess<GetObjectOutput>, &SdkError<GetObjectError>>| -> RetryKind {
//             let classifier = AwsResponseRetryClassifier::new();
//             classifier.classify_retry(res)
//         },
//     ),
