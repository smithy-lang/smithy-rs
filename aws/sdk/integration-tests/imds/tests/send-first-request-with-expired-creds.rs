/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

mod fixture;

use std::{
    convert::Infallible,
    time::{Duration, UNIX_EPOCH},
};

use aws_sdk_s3::Client;

#[tokio::test]
async fn test_request_should_be_sent_when_first_call_to_imds_returns_expired_credentials() {
    // This represents the time of a request being made, 21 Sep 2021 17:41:25 GMT.
    let time_of_request = UNIX_EPOCH + Duration::from_secs(1632246085);

    let test_fixture = fixture::TestFixture::new(
        include_str!("test-data/send-first-request-with-expired-creds.json"),
        time_of_request,
    );

    let sdk_config = test_fixture.setup().await;
    let s3_client = Client::new(&sdk_config);

    tokio::time::pause();

    // The JSON file above specifies the credentials expiry is 21 Sep 2021 11:29:29 GMT,
    // which is already invalid at the time of the request but will be made valid as the
    // code execution will go through the expiration extension.
    s3_client
        .create_bucket()
        .bucket("test-bucket-s2dhlj57-3mg8-54949-bn28-fj37tnw91is0")
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

    // The fact that the authorization of a request exists implies that the request has
    // been properly generated out of expired credentials.
    test_fixture.verify(&["authorization"]).await;
}
