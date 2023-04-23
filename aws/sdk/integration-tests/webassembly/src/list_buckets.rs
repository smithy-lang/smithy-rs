/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_sdk_s3::operation::list_buckets::ListBucketsOutput;

pub async fn s3_list_buckets() -> ListBucketsOutput {
    use aws_sdk_s3::Client;

    use crate::default_config::get_default_config;

    let shared_config = get_default_config().await;
    let client = Client::new(&shared_config);
    client.list_buckets().send().await.unwrap()
}

#[tokio::test]
pub async fn test_s3_list_buckets() {
    let result = s3_list_buckets().await;
    let buckets = result.buckets().unwrap();
    assert!(buckets.len() > 0);
}
