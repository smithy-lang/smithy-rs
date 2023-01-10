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
async fn test_request_should_be_sent_with_expired_credentials_after_imds_returns_500_during_credentials_refresh(
) {
    // This represents the time of a request being made, 21 Sep 2021 17:41:25 GMT.
    let time_of_request = UNIX_EPOCH + Duration::from_secs(1632246085);

    let mut test_fixture = fixture::TestFixture::new(
        include_str!("test-data/send-request-after-500-response-from-imds.json"),
        time_of_request,
    );

    let sdk_config = test_fixture.setup().await;
    let s3_client = Client::new(&sdk_config);

    tokio::time::pause();

    // Requests are made at 21 Sep 2021 17:41:25 GMT and 21 Sep 2021 23:41:25 GMT.
    let time_of_first_request = time_of_request;
    let time_of_second_request = UNIX_EPOCH + Duration::from_secs(1632267685);

    // The JSON file above specifies credentials will expire at between the two requests, 21 Sep 2021 23:33:13 GMT.
    // The second request will receive response 500 from IMDS but `s3_client` will eventually
    // be able to send it thanks to expired credentials held by `ImdsCredentialsProvider`.
    for (i, time_of_request_to_s3) in [time_of_first_request, time_of_second_request]
        .into_iter()
        .enumerate()
    {
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
                op.properties_mut().insert(time_of_request_to_s3);
                Result::Ok::<_, Infallible>(op)
            })
            .unwrap()
            .send()
            .await
            .unwrap();

        test_fixture.advance_time(
            time_of_second_request
                .duration_since(time_of_first_request)
                .unwrap(),
        );
    }

    // The fact that the authorization of each request exists implies that the requests have
    // been properly generated out of expired credentials.
    test_fixture.verify(&["authorization"]).await;
}
