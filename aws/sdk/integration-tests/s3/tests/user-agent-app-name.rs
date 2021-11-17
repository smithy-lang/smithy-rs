/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use aws_http::user_agent::AwsUserAgent;
use aws_sdk_s3::operation::ListObjectsV2;
use aws_sdk_s3::{AppName, Credentials, Region};

#[tokio::test]
async fn user_agent_app_name() -> Result<(), aws_sdk_s3::Error> {
    let creds = Credentials::from_keys(
        "ANOTREAL",
        "notrealrnrELgWzOk3IfjzDKtFBhDby",
        Some("notarealsessiontoken".to_string()),
    );
    let conf = aws_sdk_s3::Config::builder()
        .credentials_provider(creds)
        .region(Region::new("us-east-1"))
        .app_name(AppName::new("test-app-name").expect("valid app name")) // set app name in config
        .build();

    let op = ListObjectsV2::builder()
        .bucket("test-bucket")
        .build()
        .unwrap()
        .make_operation(&conf)
        .await
        .unwrap();
    let properties = op.properties();
    let user_agent = properties.get::<AwsUserAgent>().unwrap();
    let formatted = user_agent.aws_ua_header();

    // verify app name made it to the user agent
    assert!(
        formatted.ends_with(" app/test-app-name"),
        "'{}' didn't end with the app name",
        formatted
    );

    Ok(())
}
