/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_cbor::Encoder;

// This test simulates a server response with a cacheable field
#[test]
fn test_preserialized_data_with_cached_response() {
    // Simulate a server response with a cacheable field

    // First, create a "normal" response with all fields serialized normally
    let mut normal_encoder = Encoder::new(Vec::new());
    normal_encoder.begin_map();

    // Add a requestId field
    normal_encoder.str("requestId");
    normal_encoder.str("12345");

    // Add a userData field with a nested structure
    normal_encoder.str("userData");

    // Serialize the user data directly in the normal flow
    serialize_user_data(&mut normal_encoder);

    // End the main response
    normal_encoder.end();

    // Get the serialized bytes
    let normal_bytes = normal_encoder.into_writer();

    // Now, simulate a cached response where userData is pre-serialized

    // First, serialize just the userData to a separate buffer (simulating cached data)
    let mut user_data_encoder = Encoder::new(Vec::new());
    serialize_user_data(&mut user_data_encoder);
    let user_data_bytes = user_data_encoder.into_writer();

    // Create a new response with the cached userData
    let mut cached_encoder = Encoder::new(Vec::new());
    cached_encoder.begin_map();

    // Add the same requestId field
    cached_encoder.str("requestId");
    cached_encoder.str("12345");

    // Add the userData field with pre-serialized bytes
    cached_encoder.str("userData");
    cached_encoder
        .write_preserialized_data(&user_data_bytes)
        .unwrap();

    // End the main response
    cached_encoder.end();

    // Get the serialized bytes
    let cached_bytes = cached_encoder.into_writer();

    // Verify that both responses produce the same CBOR data
    assert_eq!(
        normal_bytes, cached_bytes,
        "Cached response should match normal response"
    );
}

// Helper function to serialize user data
// This represents the common serialization logic that would be used both for
// direct serialization and for generating cached data
fn serialize_user_data(encoder: &mut Encoder) {
    // Create a nested user data structure
    encoder.begin_map();
    encoder.str("name");
    encoder.str("John Doe");
    encoder.str("age");
    encoder.integer(30);
    encoder.str("isActive");
    encoder.boolean(true);
    encoder.end();
}
