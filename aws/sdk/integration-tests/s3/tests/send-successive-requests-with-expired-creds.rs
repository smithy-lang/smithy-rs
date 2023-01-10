/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

mod imds_fixture;

use std::{
    convert::Infallible,
    time::{Duration, UNIX_EPOCH},
};

use aws_sdk_s3::Client;

#[tokio::test]
async fn test_successive_requests_should_be_sent_with_expired_credentials_and_imds_being_called_only_once(
) {
    // This represents the time of a request being made, 21 Sep 2021 17:41:25 GMT.
    let time_of_request = UNIX_EPOCH + Duration::from_secs(1632246085);

    let test_fixture = imds_fixture::TestFixture::new(
        include_str!("test-data/send-successive-requests-with-expired-creds.json"),
        time_of_request,
    );

    let sdk_config = test_fixture.setup().await;
    let s3_client = Client::new(&sdk_config);

    tokio::time::pause();

    // The JSON file above specifies the credentials expiry is 21 Sep 2021 11:29:29 GMT,
    // which is already invalid at the time of the request but will be made valid as the
    // code execution will go through the expiration extension.
    for i in 1..=3 {
        // If IMDS were called more than once, the last `unwrap` would fail with an error looking like:
        // panicked at 'called `Result::unwrap()` on an `Err` value: ConstructionFailure(ConstructionFailure { source: CredentialsStageError { ... } })'
        // This is because the accompanying JSON file assumes that connection_id 4 (and 5) represents a request to S3,
        // not to IMDS, so its response cannot be serialized into `Credentials`.
        s3_client
            .create_bucket()
            .bucket(format!(
                "test-bucket-s2dhlj57-3mg8-54949-bn28-fj37tnw91is{}",
                i
            ))
            .customize()
            .await
            .unwrap()
            .map_operation(|mut op| {
                op.properties_mut().insert(time_of_request);
                Result::Ok::<_, Infallible>(op)
            })
            .unwrap()
            .send()
            .await
            .unwrap();
    }

    // The fact that the authorization of each request exists implies that the requests have
    // been properly generated out of expired credentials.
    test_fixture.verify(&["authorization"]).await;
}
