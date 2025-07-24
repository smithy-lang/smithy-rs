/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_cbor::Encoder;

#[test]
fn test_write_preserialized_data() {
    // Create a new encoder
    let mut encoder = Encoder::new(Vec::new());

    // Begin a map
    encoder.begin_map();

    // Add a regular field
    encoder.str("regular_field");
    encoder.str("regular_value");

    // Add a pre-serialized field
    encoder.str("preserialized_field");

    // Create pre-serialized CBOR data (a simple string "test")
    // In CBOR, a string is encoded as:
    // - Major type 3 (text string) in the first 3 bits
    // - Length in the remaining 5 bits (or following bytes for longer strings)
    // - The string data
    // For "test", the length is 4, so we use 0x64 (0x60 + 4) followed by "test"
    let preserialized_data = [0x64, b't', b'e', b's', b't'];

    // Write the pre-serialized data
    encoder
        .write_preserialized_data(&preserialized_data)
        .unwrap();

    // End the map
    encoder.end();

    // Get the final bytes
    let bytes = encoder.into_writer();

    // Since we can't easily decode and inspect the CBOR data without relying on the specific
    // minicbor API, we'll verify the output bytes directly

    // Expected CBOR encoding:
    // - 0xBF: Start indefinite-length map
    // - 0x6C: Text string of length 12
    // - "regular_field": The key
    // - 0x6D: Text string of length 13
    // - "regular_value": The value
    // - 0x71: Text string of length 17
    // - "preserialized_field": The key
    // - 0x64: Text string of length 4
    // - "test": The value
    // - 0xFF: End of indefinite-length map

    // Check that the output contains our pre-serialized data
    let preserialized_field_key = b"preserialized_field";
    let mut found_key = false;
    let mut found_value = false;

    // Find the key in the output
    for i in 0..bytes.len() - preserialized_field_key.len() {
        if bytes[i..i + preserialized_field_key.len()] == preserialized_field_key[..] {
            found_key = true;
            break;
        }
    }

    // Find the pre-serialized value in the output
    for i in 0..bytes.len() - preserialized_data.len() {
        if bytes[i..i + preserialized_data.len()] == preserialized_data[..] {
            found_value = true;
            break;
        }
    }

    assert!(found_key, "Preserialized field key not found in output");
    assert!(found_value, "Preserialized data not found in output");
}

#[test]
fn test_write_preserialized_complex_data() {
    // Create a new encoder
    let mut encoder = Encoder::new(Vec::new());

    // Begin a map
    encoder.begin_map();

    // Add a field that will contain a pre-serialized nested structure
    encoder.str("nested_structure");

    // Create pre-serialized CBOR data for a nested map with two fields
    // In CBOR:
    // - 0xA2 is a map with 2 pairs
    // - 0x63 is a text string of length 3
    // - "foo" is the first key
    // - 0x63 is a text string of length 3
    // - "bar" is the first value
    // - 0x63 is a text string of length 3
    // - "baz" is the second key
    // - 0x02 is the integer 2 (second value)
    let preserialized_map = [
        0xA2, // map with 2 pairs
        0x63, b'f', b'o', b'o', // key: "foo"
        0x63, b'b', b'a', b'r', // value: "bar"
        0x63, b'b', b'a', b'z', // key: "baz"
        0x02, // value: 2
    ];

    // Write the pre-serialized map
    encoder
        .write_preserialized_data(&preserialized_map)
        .unwrap();

    // Add another regular field
    encoder.str("regular_field");
    encoder.str("regular_value");

    // End the map
    encoder.end();

    // Get the final bytes
    let bytes = encoder.into_writer();

    // Check that the output contains our pre-serialized data
    let nested_structure_key = b"nested_structure";
    let mut found_key = false;
    let mut found_value = false;

    // Find the key in the output
    for i in 0..bytes.len() - nested_structure_key.len() {
        if bytes[i..i + nested_structure_key.len()] == nested_structure_key[..] {
            found_key = true;
            break;
        }
    }

    // Find the pre-serialized value in the output
    for i in 0..bytes.len() - preserialized_map.len() {
        if bytes[i..i + preserialized_map.len()] == preserialized_map[..] {
            found_value = true;
            break;
        }
    }

    assert!(found_key, "Nested structure key not found in output");
    assert!(found_value, "Preserialized map not found in output");
}
