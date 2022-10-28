/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_sdk_sts::{Client, Config};
use aws_smithy_http::operation::Metadata;

#[tokio::test]
async fn test_operation_metadata_is_present() {
    let client = Client::from_conf(Config::builder().build());
    // Check that operation metadata has been added to the property bag and panic if that's not the case
    let _ = client
        .get_caller_identity()
        .customize()
        .await
        .unwrap()
        .map_operation(|op| {
            let metadata = op.properties().get::<Metadata>().cloned();

            if let Some(metadata) = &metadata {
                assert_eq!("GetCallerIdentity", metadata.name());
                assert_eq!("sts", metadata.service());
            }

            metadata
                .map(|_| op)
                .ok_or("metadata was missing from the property bag")
        })
        .unwrap();
}
