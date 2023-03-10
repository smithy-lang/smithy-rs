/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_sdk_s3::error::GetObjectError;
use aws_sdk_s3::output::GetObjectOutput;
use aws_smithy_orchestrator::{BoxErr, ConfigBag, RetryStrategy};

//     retry_classifier: Arc::new(
//         |res: Result<&SdkSuccess<GetObjectOutput>, &SdkError<GetObjectError>>| -> RetryKind {
//             let classifier = AwsResponseRetryClassifier::new();
//             classifier.classify_retry(res)
//         },
//     ),

pub struct GetObjectRetryStrategy {}

impl GetObjectRetryStrategy {
    pub fn new() -> Self {
        Self {}
    }
}

impl RetryStrategy<Result<GetObjectOutput, GetObjectError>> for GetObjectRetryStrategy {
    fn should_retry(
        &self,
        res: &Result<GetObjectOutput, GetObjectError>,
        cfg: &ConfigBag,
    ) -> Result<bool, BoxErr> {
        todo!()
    }
}
