/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use aws_sdk_s3::model::BucketLocationConstraint;
use aws_sdk_s3::operation::GetBucketLocation;
use bytes::Bytes;
use smithy_http::body::SdkBody;
use smithy_http::response::ParseHttpResponse;

/// Tests that S3's GetBucketLocation call is properly customized so that the response parses correctly.
#[test]
fn get_bucket_location_deserialize() {
    let response = http::Response::builder()
        .header(
            "x-amz-id-2",
            "H5V7t/DQ/aDDv8izxFO9oH6I+wy84lS9jWyULF2JugGC0wx0QzzLKqeyMQ3STJRk2tJvf5PhTa4="
        )
        .header("x-amz-request-id", "WEFJEDM657CYJPGW")
        .status(200)
        .body(Bytes::from(r#"<?xml version="1.0" encoding="UTF-8"?>
        <LocationConstraint xmlns="http://s3.amazonaws.com/doc/2006-03-01/">us-west-2</LocationConstraint>"#))
        .unwrap();

    let parser = GetBucketLocation::new();
    let parsed =
        <GetBucketLocation as ParseHttpResponse<SdkBody>>::parse_loaded(&parser, &response)
            .unwrap();
    assert_eq!(
        Some(BucketLocationConstraint::UsWest2),
        parsed.location_constraint
    );
}
