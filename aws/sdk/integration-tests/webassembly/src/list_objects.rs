/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_sdk_s3::operation::list_objects_v2::ListObjectsV2Output;

async fn s3_list_objects() -> ListObjectsV2Output {
    use aws_sdk_s3::Client;
    use crate::default_config::get_default_config;

    let shared_config = get_default_config().await;
    let client = Client::new(&shared_config);
    let operation = client
        .list_objects_v2()
        .bucket("nara-national-archives-catalog")
        .delimiter("/")
        .prefix("authority-records/organization/")
        .max_keys(5)
        .customize()
        .await
        .unwrap();
    operation.send().await.unwrap()
}

#[tokio::test]
pub async fn test_s3_list_objects() {
    let result = s3_list_objects().await;
    println!("result: {:?}", result);
    let objects = result.contents().unwrap();
    assert!(objects.len() > 1);
}
