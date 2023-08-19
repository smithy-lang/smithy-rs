/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_sdk_s3::operation::list_objects_v2::ListObjectsV2Output;

pub async fn s3_list_objects() -> ListObjectsV2Output {
    use aws_sdk_s3::Client;

    use crate::default_config::get_default_config;

    let shared_config = get_default_config().await;
    let client = Client::new(&shared_config);
    let mut operation = client
        .list_objects_v2()
        .bucket("smithsonian-open-access")
        .delimiter("/")
        .prefix("media/")
        .max_keys(5)
        .customize()
        .await
        .unwrap();
    operation.map_operation(make_unsigned).unwrap().send().await.unwrap()
}

fn make_unsigned<O, Retry>(
    mut operation: aws_smithy_http::operation::Operation<O, Retry>,
) -> Result<aws_smithy_http::operation::Operation<O, Retry>, std::convert::Infallible> {
    {
        let mut props = operation.properties_mut();
        let mut signing_config = props
            .get_mut::<aws_sig_auth::signer::OperationSigningConfig>()
            .expect("has signing_config");
        signing_config.signing_requirements = aws_sig_auth::signer::SigningRequirements::Disabled;
    }

    Ok(operation)
}

#[tokio::test]
pub async fn test_s3_list_objects() {
    let result = s3_list_objects().await;
    println!("result: {:?}", result);
    let objects = result.contents().unwrap();
    assert!(objects.len() > 1);
}
