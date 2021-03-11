/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use kms::error::{CreateAliasError, CreateAliasErrorKind, LimitExceededError};
use kms::operation::CreateAlias;
use kms::output::{CreateAliasOutput, GenerateRandomOutput};
use kms::Blob;
use smithy_http::middleware::ResponseBody;
use smithy_http::result::{SdkError, SdkSuccess};
use smithy_http::retry::ClassifyResponse;
use smithy_types::retry::{ErrorKind, RetryKind};

#[test]
fn validate_sensitive_trait() {
    let output = GenerateRandomOutput::builder()
        .plaintext(Blob::new("some output"))
        .build();
    assert_eq!(
        format!("{:?}", output),
        "GenerateRandomOutput { plaintext: \"*** Sensitive Data Redacted ***\" }"
    );
}

#[test]
fn errors_are_retryable() {
    let kind = CreateAliasErrorKind::LimitExceededError(LimitExceededError::builder().build());
    let err = CreateAliasError::new(kind, Default::default());
    assert_eq!(err.code(), Some("LimitExceededException"));
    let conf = kms::Config::builder().build();

    let op = CreateAlias::builder().build(&conf);
    let err = Result::<SdkSuccess<CreateAliasOutput>, SdkError<CreateAliasError>>::Err(
        SdkError::ServiceError {
            raw: http::Response::builder()
                .body(ResponseBody::from_static("resp"))
                .unwrap(),
            err,
        },
    );
    let retry_kind = op.retry_policy().classify(err.as_ref());
    assert_eq!(retry_kind, RetryKind::Error(ErrorKind::ThrottlingError));
}
